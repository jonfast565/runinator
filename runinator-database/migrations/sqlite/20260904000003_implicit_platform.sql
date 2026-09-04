DELETE FROM role_assignments WHERE principal_kind = 'user' AND scope_key = 'platform' AND role <> 'admin';
CREATE TRIGGER human_platform_role_insert BEFORE INSERT ON role_assignments
WHEN NEW.principal_kind = 'user' AND NEW.scope_key = 'platform' AND NEW.role <> 'admin'
BEGIN SELECT RAISE(ABORT, 'human platform access must be admin or absent'); END;
CREATE TRIGGER human_platform_role_update BEFORE UPDATE ON role_assignments
WHEN NEW.principal_kind = 'user' AND NEW.scope_key = 'platform' AND NEW.role <> 'admin'
BEGIN SELECT RAISE(ABORT, 'human platform access must be admin or absent'); END;
