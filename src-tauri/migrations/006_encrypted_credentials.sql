-- 旧系统钥匙串不再读取；保留账号、自动登录及启用偏好，仅清除凭据存在标志。
UPDATE connections SET data=json_set(data, '$.hasCredential', json('false'));
UPDATE entity_history SET data=json_set(data, '$.hasCredential', json('false'))
WHERE entity_type='connections';
UPDATE mail_accounts SET data=json_set(data,
    '$.hasImapCredential', json('false'), '$.hasSmtpCredential', json('false'));
UPDATE settings SET value=json_set(value, '$.config.hasCredential', json('false'))
WHERE key='dingtalk-caldav';
UPDATE settings SET value=json_set(value, '$.hasSecrets', json('false'))
WHERE key GLOB 'mcp.connection.*';

-- Rust 在同一事务中加密现有数据并写入本机密钥校验标记；中断后继续清理旧页。
INSERT INTO settings(key,value) VALUES('credential-encryption-cleanup-pending','true');
