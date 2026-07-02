export interface User {
  id: string | null;
  username: string;
  email?: string | null;
  is_admin: boolean;
  disabled: boolean;
  created_at: string;
  updated_at: string;
}
