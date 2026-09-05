INSERT INTO coordinator_refusals
    (kind, subject, worker_id, session_id, reason,
     first_observed_at, last_observed_at, observations, cleared_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1, NULL)
ON CONFLICT(kind, subject) DO UPDATE SET
    last_observed_at = excluded.last_observed_at,
    observations = CASE
        WHEN coordinator_refusals.cleared_at IS NULL
          AND coordinator_refusals.worker_id IS excluded.worker_id
          AND coordinator_refusals.session_id IS excluded.session_id
        THEN coordinator_refusals.observations + 1
        ELSE 1
    END,
    -- A cleared refusal happening again is a new occurrence.
    first_observed_at = CASE
        WHEN coordinator_refusals.cleared_at IS NULL
          AND coordinator_refusals.worker_id IS excluded.worker_id
          AND coordinator_refusals.session_id IS excluded.session_id
        THEN coordinator_refusals.first_observed_at
        ELSE excluded.first_observed_at
    END,
    reason = excluded.reason,
    worker_id = excluded.worker_id,
    session_id = excluded.session_id,
    cleared_at = NULL
