-- params: uuid
SELECT *
  FROM holdout_evaluations
 WHERE holdout_id = $1
 ORDER BY evaluation_num ASC;
