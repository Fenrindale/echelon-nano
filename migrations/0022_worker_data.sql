-- Generated business artifacts are bounded by storage resources, not a hidden
-- byte quota. Operator upload limits remain enforced at the API boundary.
-- This changes no existing bytes, hashes, Work Orders or immutable histories.
ALTER TABLE artifacts DROP CONSTRAINT artifacts_body_check;
