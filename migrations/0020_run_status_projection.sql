-- Project Run lifecycle status from durable execution history.
-- Only Runs still projected as QUEUED or RUNNING are updated; an existing
-- terminal Run is never overwritten and no Campaign status is inferred.

UPDATE runs
SET status = (
  SELECT CASE g.outcome
    WHEN 'ACCEPTED' THEN 'ACCEPTED'
    WHEN 'REPAIR_REQUIRED' THEN 'REPAIR_REQUIRED'
    ELSE 'REJECTED'
  END
  FROM candidates cd
  JOIN attempts a ON a.id=cd.attempt_id
  JOIN gate_decisions g ON g.candidate_id=cd.id
  WHERE a.run_id=runs.id
  ORDER BY g.decided_at DESC, cd.id DESC
  LIMIT 1
)
WHERE runs.status IN ('QUEUED','RUNNING')
  AND EXISTS (
    SELECT 1 FROM candidates cd
    JOIN attempts a ON a.id=cd.attempt_id
    JOIN gate_decisions g ON g.candidate_id=cd.id
    WHERE a.run_id=runs.id
  );

UPDATE runs
SET status='CANCELLED'
WHERE runs.status IN ('QUEUED','RUNNING')
  AND EXISTS (
    SELECT 1 FROM campaigns c
    WHERE c.id=runs.campaign_id AND c.status='CANCELLED'
  );

UPDATE runs
SET status='FAILED'
WHERE runs.status IN ('QUEUED','RUNNING')
  AND EXISTS (
    SELECT 1 FROM attempts a
    WHERE a.run_id=runs.id AND a.status='FAILED'
  )
  AND NOT EXISTS (
    SELECT 1 FROM candidates cd
    JOIN attempts a ON a.id=cd.attempt_id
    JOIN gate_decisions g ON g.candidate_id=cd.id
    WHERE a.run_id=runs.id
  );
