# AWS\+腾讯云 Serverless 鉴权方案（API Key \+ S3/COS \+ Gzip 自动解压）

# 一、方案核心目标

- 客户端通过 **Authorization: Bearer \<token\>** 鉴权，仅能访问指定前缀路径的文件（如 S3://bucket/prefix/path/\*、COS://bucket/prefix/path/\*）

- 纯 Serverless 架构，0 服务器维护、自动扩缩容，双云（AWS\+腾讯云）1:1 对齐

- 文件以 Gzip 压缩存储，节省流量和成本，客户端下载后自动解压（无感体验）

- 最优交互：客户端仅发 1 次请求，通过 302 重定向直连存储，兼顾简单、快速、低成本

# 二、整体架构（双云统一）

## 2\.1 核心架构流程

1. 客户端发送请求（带 `Authorization: Bearer \<token\>` 头部），访问 API 网关的指定路径（如 /get/\{path\}）

2. API 网关将请求转发至后端无服务器函数（Lambda/SCF）

3. 函数解析 Bearer Token 并校验其绑定的权限：判断请求路径是否在允许的前缀范围内

4. 鉴权通过后，函数生成 S3/COS 临时签名 URL（15分钟有效）

5. 函数返回 302 重定向，客户端自动跳转至临时 URL 下载文件

6. S3/COS 返回 Gzip 压缩文件，客户端（浏览器/App/下载工具）自动识别并解压，无需手动操作

## 2\.2 双云产品对应表

|用途|AWS|腾讯云|
|---|---|---|
|对象存储（存储 Gzip 压缩文件）|S3|对象存储（COS）|
|无服务器函数（鉴权\+生成临时URL）|Lambda|云函数（SCF）|
|API 入口（Bearer Token 在函数内校验）|API Gateway|API 网关|
|权限存储（Token\-前缀映射）|DynamoDB（生产）/ 函数内配置（测试）|CDB/Redis（生产）/ 函数内配置（测试）|

# 三、核心实现代码（双云可直接复制上线）

## 3\.1 AWS 实现（S3 \+ Lambda \+ API Gateway）

### 3\.1\.1 Lambda 鉴权\+302重定向代码（Python）

```python
import boto3

# 权限配置（测试用，生产建议迁移到 DynamoDB）
API_KEY_PERMISSIONS = {
    "api_key_abc123": {
        "bucket": "your-bucket-name",  # 替换为你的 S3 桶名
        "allowed_prefixes": ["userA/", "public/"]  # 允许访问的路径前缀
    },
    "api_key_xyz456": {
        "bucket": "your-bucket-name",
        "allowed_prefixes": ["userB/"]
    }
}

# 初始化 S3 客户端
s3 = boto3.client('s3')

def _extract_bearer(headers):
    for k, v in headers.items():
        if k.lower() == 'authorization' and isinstance(v, str) and v[:7].lower() == 'bearer ':
            return v[7:].strip()
    return ''

def lambda_handler(event, context):
    # 1. 解析 Authorization: Bearer <token> 头部和请求路径
    api_key = _extract_bearer(event.get('headers', {}))
    path = event['pathParameters']['path']  # 路径参数，如 userA/photo.jpg

    # 2. Token 有效性校验
    if not api_key or api_key not in API_KEY_PERMISSIONS:
        return {"statusCode": 401, "headers": {"WWW-Authenticate": "Bearer"}, "body": "Invalid or missing Bearer token"}
    
    # 3. 路径前缀权限校验
    config = API_KEY_PERMISSIONS[api_key]
    bucket = config['bucket']
    allowed_prefixes = config['allowed_prefixes']
    if not any(path.startswith(prefix) for prefix in allowed_prefixes):
        return {"statusCode": 403, "body": "Access Denied: Path not allowed"}

    # 4. 生成 S3 临时签名 URL（15分钟有效）
    presigned_url = s3.generate_presigned_url(
        ClientMethod='get_object',
        Params={'Bucket': bucket, 'Key': path + '.gz'},  # 拼接 .gz 后缀（对应压缩文件）
        ExpiresIn=900  # 有效时间：900秒（15分钟）
    )

    # 5. 返回 302 重定向，客户端自动跳转至临时 URL
    return {
        "statusCode": 302,
        "headers": {
            "Location": presigned_url
        }
    }
```

### 3\.1\.2 API Gateway 配置

1. 创建 API，授权方式设置为 **NONE**（鉴权由 Lambda 解析 Bearer Token 完成），绑定 Lambda 函数

2. 设置路径：`/get/\{path\}`（\{path\} 为路径参数，对应文件路径，如 userA/photo\.jpg）

3. 部署 API，客户端请求示例：
        `GET https://你的API网关域名/get/userA/photo\.jpg
Header: Authorization: Bearer api\_key\_abc123`

## 3\.2 腾讯云实现（COS \+ SCF \+ API 网关）

### 3\.2\.1 SCF 鉴权\+302重定向代码（Python）

```python
from qcloud_cos import CosConfig, CosS3Client

# 权限配置（测试用，生产建议迁移到 CDB/Redis）
API_KEY_PERMISSIONS = {
    "api_key_tc_123": {
        "bucket": "your-bucket-12345",  # 替换为你的 COS 桶名（不带后缀）
        "region": "ap-beijing",  # 替换为你的 COS 地域（如 ap-shanghai）
        "allowed_prefixes": ["userA/", "public/"]
    }
}

# 初始化 COS 客户端（使用函数临时密钥，无需硬编码密钥）
def get_cos_client(region):
    config = CosConfig(Region=region)
    return CosS3Client(config)

def _extract_bearer(headers):
    for k, v in headers.items():
        if k.lower() == 'authorization' and isinstance(v, str) and v[:7].lower() == 'bearer ':
            return v[7:].strip()
    return ''

def main_handler(event, context):
    # 1. 解析 Authorization: Bearer <token> 头部和请求路径
    api_key = _extract_bearer(event.get('headers', {}))
    path = event['pathParameters']['path']  # 如 userA/photo.jpg

    # 2. Token 校验
    if not api_key or api_key not in API_KEY_PERMISSIONS:
        return {"statusCode": 401, "headers": {"WWW-Authenticate": "Bearer"}, "body": "Invalid or missing Bearer token"}
    
    # 3. 路径前缀校验
    config = API_KEY_PERMISSIONS[api_key]
    bucket = config['bucket']
    region = config['region']
    allowed_prefixes = config['allowed_prefixes']
    if not any(path.startswith(prefix) for prefix in allowed_prefixes):
        return {"statusCode": 403, "body": "Access Denied: Path not allowed"}

    # 4. 生成 COS 临时签名 URL（15分钟有效）
    cos_client = get_cos_client(region)
    presigned_url = cos_client.get_presigned_url(
        Bucket=bucket,
        Key=path + '.gz',  # 拼接 .gz 后缀（对应压缩文件）
        Expired=900  # 15分钟有效
    )

    # 5. 返回 302 重定向
    return {
        "statusCode": 302,
        "headers": {
            "Location": presigned_url
        }
    }
```

### 3\.2\.2 API 网关配置

1. 创建 API，授权方式为 **NONE**（Bearer Token 由 SCF 函数解析），触发 SCF 函数

2. 设置路径：`/get/\{path\}`，与 AWS 保持一致

3. 客户端请求方式与 AWS 完全相同（`Authorization: Bearer \<token\>`），无需修改代码

# 四、Gzip 压缩存储 \+ 自动解压配置（核心）

## 4\.1 核心原理

文件以 Gzip 格式压缩存储到 S3/COS，上传时设置 2 个关键 HTTP 头，客户端下载时会自动识别并解压，全程无感：

- `Content\-Encoding: gzip`：告诉客户端文件是 Gzip 压缩格式，需自动解压

- `Content\-Type: 原始文件类型`（如 image/jpeg、application/json）：确保客户端正确识别解压后的文件类型

## 4\.2 自动化配置（无需手工设置 Header）

无需每次上传手动设置 Header，通过脚本或存储规则自动完成，分两种方式：

### 4\.2\.1 方式1：上传时自动压缩\+自动设置 Header（脚本一键）

#### AWS S3 上传脚本（一行命令）

```bash
# 原文件：file.jpg → 自动压缩为 Gzip → 上传到 S3 → 自动设置 Header
gzip -c file.jpg | aws s3 cp - s3://your-bucket-name/userA/file.jpg.gz \
  --content-type "image/jpeg" \
  --content-encoding "gzip"
```

#### 腾讯云 COS 上传脚本

```bash
# 原文件：file.jpg → 压缩为 Gzip → 上传到 COS → 自动设置 Header
gzip -c file.jpg > file.jpg.gz
coscmd put file.jpg.gz userA/file.jpg.gz \
  --content-type "image/jpeg" \
  --content-encoding "gzip"
```

### 4\.2\.2 方式2：存储自动规则（永久生效，任何上传方式都适用）

#### AWS S3 自动规则配置

1. 登录 S3 控制台，进入目标桶 → 点击「管理」→ 找到「元数据规则」→ 点击「创建规则」

2. 设置规则条件：`后缀（Suffix）` 为`\.gz`

3. 设置操作：添加两个元数据：
        

    - 键：Content\-Encoding，值：gzip

    - 键：Content\-Type，值：自动识别（或手动指定对应类型）

4. 保存规则后，所有上传的 \.gz 文件都会自动带上正确的 Header，无需手动干预

#### 腾讯云 COS 自动规则配置

1. 登录 COS 控制台，进入目标桶 → 点击「基础配置」→ 找到「自定义头部」→ 点击「添加规则」

2. 设置规则：
        

    - 文件名后缀：\.gz

    - 响应头：添加 Content\-Encoding = gzip

3. 保存后，所有 \.gz 文件上传后会自动带上 Header，客户端自动解压

## 4\.3 客户端行为（全自动，无需任何改动）

- 浏览器：自动跳转 → 识别 Gzip → 后台解压 → 直接显示图片/渲染文本

- App/桌面客户端：主流网络库（OkHttp、Axios 等）默认自动解压，拿到原始文件

- 手动下载（curl/wget）：直接请求，得到解压后的原始文件，无需手动解压

# 五、成本对比（最优选择）

|方案|客户端请求次数|流量费用|性能|推荐度|
|---|---|---|---|---|
|返回 JSON URL（二次请求）|2次|低（仅存储流量）|高|⭐⭐⭐|
|函数代理直出（一次请求）|1次|高（函数流量\+计算费）|一般|⭐⭐|
|302 重定向 \+ Gzip（本方案）|1次|最低（Gzip 省 30%\-80% 流量）|最高（存储直连）|⭐⭐⭐⭐⭐（推荐）|

# 六、生产级增强建议（可选）

1. 权限存储：将函数内的 Bearer Token 配置，迁移到 DynamoDB（AWS）/ CDB/Redis（腾讯云），支持动态更新、密钥轮换

2. 限流防护：在 API 网关开启限流，防止恶意请求攻击

3. 日志审计：开启 API 网关、函数、存储的访问日志，便于排查问题

4. CORS 配置：在 API 网关设置跨域规则，支持前端直调

5. HTTPS 强制：双云 API 网关默认支持 HTTPS，确保传输安全

# 七、上线步骤（简化版）

1. 在 S3/COS 配置自动 Header 规则（方式2），确保 \.gz 文件自动带对头部

2. 将文件用 Gzip 压缩（gzip \-9 文件名），上传到对应路径（如 userA/photo\.jpg\.gz）

3. 部署双云的无服务器函数（Lambda/SCF），复制上述对应代码并修改桶名、地域等配置

4. 配置 API 网关，开启 API Key 认证，绑定函数并部署

5. 客户端发送请求（带 `Authorization: Bearer \<token\>` 头部），测试自动重定向、自动解压功能

> （注：文档部分内容可能由 AI 生成）
