// a small closed enum served for the frontend's <select> controls (gate kind, edge match kind,
// branch policy, setting kind).
export interface EnumOptionMetadata {
  value: string;
  label: string;
  description?: string | null;
}

export interface EnumCatalogMetadata {
  name: string;
  options: EnumOptionMetadata[];
}
