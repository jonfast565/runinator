// Actions are discovered from `/authz/catalog`; the client intentionally does not mirror the
// server's fixed catalog. This string brand keeps navigation and controls typed without creating a
// second source of truth.
export type Action = string;
