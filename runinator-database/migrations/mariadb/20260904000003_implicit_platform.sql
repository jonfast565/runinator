DELETE FROM role_assignments WHERE principal_kind = 'user' AND scope_key = 'platform' AND role <> 'admin';
ALTER TABLE role_assignments ADD CONSTRAINT human_platform_role CHECK (principal_kind <> 'user' OR scope_key <> 'platform' OR role = 'admin');
