-- the wdl `compute { }` block was renamed to `do { }`. console cells record the classifier's
-- verdict as a plain string, so existing rows still say 'compute'; rewrite them to the new name
-- rather than teaching the enum a legacy alias it would carry forever.
UPDATE console_cells SET kind = 'do' WHERE kind = 'compute';
