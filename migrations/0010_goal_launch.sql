ALTER TABLE tasks ADD COLUMN launch_key uuid;
CREATE UNIQUE INDEX task_goal_launch_key ON tasks(tenant_id,launch_key) WHERE launch_key IS NOT NULL;
CREATE FUNCTION guard_goal_launch() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF OLD.launch_key IS NOT NULL AND (NEW.launch_key IS DISTINCT FROM OLD.launch_key OR NEW.goal<>OLD.goal)
 THEN RAISE EXCEPTION 'immutable goal launch identity'; END IF;
 RETURN NEW; END; $$;
CREATE TRIGGER task_goal_launch_guard BEFORE UPDATE ON tasks FOR EACH ROW EXECUTE FUNCTION guard_goal_launch();
