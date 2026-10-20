ALTER TABLE tasks ADD COLUMN launch_manifest jsonb NOT NULL DEFAULT '[]';
CREATE OR REPLACE FUNCTION guard_goal_launch() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF OLD.launch_key IS NOT NULL AND (NEW.launch_key IS DISTINCT FROM OLD.launch_key OR NEW.goal<>OLD.goal OR NEW.launch_manifest<>OLD.launch_manifest)
 THEN RAISE EXCEPTION 'immutable goal launch identity'; END IF;
 RETURN NEW; END; $$;
