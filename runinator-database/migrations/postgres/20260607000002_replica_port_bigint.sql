-- align replicas.port with the i64 mapper read; postgres INTEGER (INT4) did not
-- decode as Option<i64> (INT8). matches the mysql BIGINT column type.
ALTER TABLE replicas ALTER COLUMN port TYPE BIGINT;
