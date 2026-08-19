export interface User {
  id: string | null;
  username: string;
  email?: string | null;
  platform_role: "admin" | "operator" | "auditor" | "member";
  disabled: boolean;
  created_at: string;
  updated_at: string;
}
