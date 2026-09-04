SELECT refusal.kind, refusal.subject, refusal.worker_id, worker.name,
                    refusal.reason, refusal.first_observed_at, refusal.last_observed_at,
                    refusal.observations
             FROM coordinator_refusals refusal
             LEFT JOIN worker_profiles worker ON worker.id = refusal.worker_id
             WHERE refusal.cleared_at IS NULL
               AND (refusal.kind NOT IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR refusal.subject NOT LIKE 'decision:%'
                    OR NOT EXISTS (SELECT 1 FROM decision_requests decision WHERE decision.id = substr(refusal.subject, 10))
                    OR EXISTS (
                        SELECT 1 FROM decision_deliveries delivery
                        JOIN decision_requests decision ON decision.id = delivery.decision_id
                        WHERE decision.id = substr(refusal.subject, 10) AND decision.state = 'resolved'
                          AND delivery.state IN ('queued','dispatching')
                          AND (refusal.worker_id IS NULL OR delivery.worker_id = refusal.worker_id)
                          AND (refusal.session_id IS NULL OR delivery.session_id = refusal.session_id)
                    ))
               AND (refusal.kind NOT IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR refusal.subject NOT LIKE 'task-outcome:%'
                    OR NOT EXISTS (SELECT 1 FROM tasks task WHERE task.id = substr(refusal.subject, 14))
                    OR EXISTS (
                        SELECT 1 FROM task_outcome_deliveries outcome
                        JOIN tasks task ON task.id = outcome.task_id
                        WHERE task.id = substr(refusal.subject, 14)
                          AND outcome.state IN ('queued','dispatching')
                          AND task.removed_at IS NULL AND task.state = outcome.target_state
                          AND (refusal.worker_id IS NULL OR outcome.recipient_worker_id = refusal.worker_id)
                          AND (refusal.session_id IS NULL OR outcome.session_id = refusal.session_id)
                    ))
               AND (refusal.kind NOT IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR refusal.subject NOT LIKE 'task-outcome:%'
                    OR NOT EXISTS (SELECT 1 FROM tasks task WHERE task.id = substr(refusal.subject, 14))
                    OR EXISTS (
                        SELECT 1 FROM task_outcome_deliveries outcome
                        JOIN tasks task ON task.id = outcome.task_id
                        WHERE task.id = substr(refusal.subject, 14)
                          AND outcome.state IN ('queued','dispatching')
                          AND task.removed_at IS NULL AND task.state = outcome.target_state
                          AND (refusal.worker_id IS NULL OR outcome.recipient_worker_id = refusal.worker_id)
                          AND (refusal.session_id IS NULL OR outcome.session_id = refusal.session_id)
                    ))
               AND (refusal.kind NOT IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR refusal.subject NOT LIKE 'outcome-delivery:%'
                    OR EXISTS (
                        SELECT 1 FROM task_outcome_deliveries outcome
                        JOIN tasks task ON task.id = outcome.task_id
                        WHERE outcome.id = substr(refusal.subject, 18)
                          AND outcome.state IN ('queued','dispatching')
                          AND task.removed_at IS NULL AND task.state = outcome.target_state
                          AND outcome.recipient_worker_id = refusal.worker_id
                          AND outcome.session_id = refusal.session_id
                    ))
               AND (refusal.kind NOT IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR refusal.subject != 'queen-review'
                    OR EXISTS (SELECT 1 FROM queen_automation WHERE id = 1 AND
                        (run_id IS NULL OR state IN ('queued','delivering'))))
               AND (refusal.kind NOT IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR refusal.subject NOT LIKE 'queen-run:%'
                    OR EXISTS (
                        SELECT 1 FROM queen_automation automation
                        JOIN worker_profiles queen ON queen.role = 'queen' AND queen.id = refusal.worker_id
                        WHERE automation.id = 1 AND automation.run_id = substr(refusal.subject, 11)
                          AND automation.state IN ('queued','delivering')
                          AND automation.delivery_session_id = refusal.session_id
                    ))
               AND (refusal.kind NOT IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR refusal.subject NOT LIKE 'task-dispatch:%'
                    OR EXISTS (
                        SELECT 1 FROM task_dispatches dispatch
                        JOIN task_assignments assignment ON assignment.id = dispatch.assignment_id
                        JOIN tasks task ON task.id = dispatch.task_id
                        WHERE dispatch.assignment_id = substr(refusal.subject, 15)
                          AND task.removed_at IS NULL AND task.state IN ('ready', 'active')
                          AND dispatch.state IN ('queued', 'dispatching') AND assignment.released_at IS NULL
                          AND dispatch.worker_id = refusal.worker_id
                          AND assignment.worker_session_id = refusal.session_id
                    ))
               -- A known task's briefing hold is obsolete once its live
               -- assignment no longer has a pending dispatch. Keep unknown
               -- legacy subjects as unresolved rather than guessing recovery.
               AND (refusal.kind NOT IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR refusal.subject NOT LIKE 'task-brief:%'
                    OR NOT EXISTS (SELECT 1 FROM tasks task WHERE task.id = substr(refusal.subject, 12))
                    OR EXISTS (
                        SELECT 1 FROM task_dispatches dispatch
                        JOIN task_assignments assignment ON assignment.id = dispatch.assignment_id
                        JOIN tasks task ON task.id = dispatch.task_id
                        WHERE task.id = substr(refusal.subject, 12)
                          AND task.removed_at IS NULL AND task.state IN ('ready', 'active')
                          AND dispatch.state IN ('queued', 'dispatching') AND assignment.released_at IS NULL
                          AND (refusal.worker_id IS NULL OR dispatch.worker_id = refusal.worker_id)
                          AND (refusal.session_id IS NULL OR assignment.worker_session_id = refusal.session_id)
                    ))
               -- A known ended session cannot still hold terminal input.
               -- Preserve unbound legacy evidence and non-terminal recovery.
               AND (refusal.kind NOT IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR refusal.session_id IS NULL
                    OR NOT EXISTS (
                        SELECT 1 FROM worker_sessions session
                        WHERE session.session_id = refusal.session_id
                          AND session.ended_at IS NOT NULL
                    ))
               AND ?1 - refusal.first_observed_at >= ?2
               AND (refusal.kind IN ('delivery_held_open_prompt', 'delivery_held_unsent_text')
                    OR ?1 - refusal.last_observed_at <= ?3)
             ORDER BY refusal.first_observed_at LIMIT 257
