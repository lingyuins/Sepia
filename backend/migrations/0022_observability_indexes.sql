-- The console request list resolves each row through correlated subqueries on
-- attempts.request_id and orders by requests.created_at; without these indexes
-- every refresh full-scans both tables, and the long read starved inference
-- writes under the DELETE journal (500 "observability unavailable").
CREATE INDEX IF NOT EXISTS attempts_request ON attempts (request_id);
CREATE INDEX IF NOT EXISTS requests_created ON requests (created_at);