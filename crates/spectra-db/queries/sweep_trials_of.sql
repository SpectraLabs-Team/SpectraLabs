-- params: uuid
SELECT *
  FROM optimization_trials
 WHERE sweep_id = $1
 ORDER BY trial_num ASC;
