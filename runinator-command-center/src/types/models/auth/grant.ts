import type { PermissionLevel, PrincipalType } from "./permission";

export interface Grant {
  id: string | null;
  resource_type: string;
  resource_id: string;
  principal_type: PrincipalType;
  principal_id: string;
  permission: PermissionLevel;
  created_at: string;
}
