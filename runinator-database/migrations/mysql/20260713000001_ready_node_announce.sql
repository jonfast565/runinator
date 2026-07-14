-- lease column for the wake publisher: a pending ready node is announced on the wake channel once
-- per lease window instead of once per publisher tick. broker backends without in-flight dedupe
-- (rabbitmq, kafka) otherwise accumulate duplicate wakes without bound.
ALTER TABLE workflow_ready_nodes ADD COLUMN announced_until BIGINT NULL;
