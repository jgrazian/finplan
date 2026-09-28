-- Courses of action (api::suggestions): a suggestion offers up to four paths,
-- each one to four ordered steps, each step its own batch of changes. The user
-- follows one path, whole or step by step; `applied_path` is the path being
-- followed once any step of it is applied, and `created_json` the entities
-- its applied steps created, by key, so later steps can refer to them.
--
-- An existing suggestion's single batch becomes path "a" with one step "a".
ALTER TABLE suggestions ADD COLUMN paths_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE suggestions ADD COLUMN applied_path TEXT;
ALTER TABLE suggestions ADD COLUMN created_json TEXT NOT NULL DEFAULT '{}';

UPDATE suggestions
   SET paths_json = json_array(json_object(
         'key', 'a',
         'label', 'Suggested change',
         'reasoning', NULL,
         'recommended', json('true'),
         'steps', json_array(json_object(
             'key', 'a',
             'title', CASE WHEN length(title) <= 80 THEN title
                           ELSE rtrim(substr(title, 1, 79)) || '…' END,
             'reasoning', NULL,
             'changes', json(changes_json),
             'diff', json(diff_json),
             'applied', json(CASE WHEN status = 'applied' THEN 'true' ELSE 'false' END),
             'applied_at', CASE WHEN status = 'applied' THEN resolved_at END)),
         'estimate', json(estimate_json),
         'check', json(check_json)))
 WHERE changes_json <> '[]';

UPDATE suggestions SET applied_path = 'a' WHERE status = 'applied' AND paths_json <> '[]';

ALTER TABLE suggestions DROP COLUMN changes_json;
ALTER TABLE suggestions DROP COLUMN diff_json;
ALTER TABLE suggestions DROP COLUMN estimate_json;
ALTER TABLE suggestions DROP COLUMN check_json;
