from qcloud_cos import CosConfig, CosS3Client

API_KEY_PERMISSIONS = {
    "api_key_tc_123": {
        "bucket": "your-bucket-12345",
        "region": "ap-beijing",
        "allowed_prefixes": ["userA/", "public/"]
    }
}

def get_cos_client(region):
    config = CosConfig(Region=region)
    return CosS3Client(config)

def main_handler(event, _context):
    api_key = event['headers'].get('x-api-key', '')
    path = event['pathParameters']['path']

    if api_key not in API_KEY_PERMISSIONS:
        return {"statusCode": 403, "body": "Invalid API Key"}

    config = API_KEY_PERMISSIONS[api_key]
    bucket = config['bucket']
    region = config['region']
    allowed_prefixes = config['allowed_prefixes']

    if not any(path.startswith(prefix) for prefix in allowed_prefixes):
        return {"statusCode": 403, "body": "Access Denied: Path not allowed"}

    cos_client = get_cos_client(region)
    presigned_url = cos_client.get_presigned_url(
        Bucket=bucket,
        Key=path + '.gz',
        Expired=900
    )

    return {
        "statusCode": 302,
        "headers": {
            "Location": presigned_url
        }
    }
