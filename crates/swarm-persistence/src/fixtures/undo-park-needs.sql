-- Test-only: a park recorded before schema 206, worded `You took on "..."
-- yourself.`, with the decision its activity row names. Rows are created here
-- so the step can be undone on an empty database too; existing ones are kept.
INSERT OR IGNORE INTO operators (id, display_name)
VALUES ('00000000-0000-7000-8000-000000000206', 'Park fixture operator');
INSERT OR IGNORE INTO hives (id, name, operator_id)
VALUES ('00000000-0000-7000-8000-000000000207', 'Park fixture Hive',
        '00000000-0000-7000-8000-000000000206');
INSERT OR IGNORE INTO worker_profiles (id, name, role, provider, workspace, autostart, position, hive_id)
VALUES ('00000000-0000-7000-8000-000000000208', 'Park fixture worker', 'worker', 'claude_code',
        '/workspace/park-fixture', 0, 206, '00000000-0000-7000-8000-000000000207');
INSERT OR IGNORE INTO tasks (id, title, workspace, state, hive_id)
VALUES ('00000000-0000-7000-8000-000000000209', 'Park fixture task', '/workspace/park-fixture',
        'blocked', '00000000-0000-7000-8000-000000000207');
INSERT OR IGNORE INTO decision_requests
    (id, hive_id, requesting_worker_id, task_id, kind, urgency, title, reason, risk, evidence,
     suggested_action, allowed_actions)
VALUES ('00000000-0000-7000-8000-00000000020a', '00000000-0000-7000-8000-000000000207',
        '00000000-0000-7000-8000-000000000208', '00000000-0000-7000-8000-000000000209',
        'input', 'normal', 'Arrange the fixture', 'r', 'r', 'e', 'I will do it',
        '["I will do it"]');
INSERT OR IGNORE INTO task_activity (sequence, task_id, kind, to_state, note, actor_kind)
VALUES (2060000, '00000000-0000-7000-8000-000000000209', 'state_changed', 'blocked',
        'Parked by the operator''s own answer "I will do it" on decision '
            || '00000000-0000-7000-8000-00000000020a, which is an action they carry out themselves.',
        'operator');
INSERT OR IGNORE INTO task_waits (task_id, block_sequence, kind, needs, created_at, actor_kind)
VALUES ('00000000-0000-7000-8000-000000000209', 2060000, 'operator_action',
        'You took on "I will do it" yourself.', 1, 'operator');
