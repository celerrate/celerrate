<?php
isset($a);
isset($a, $b[0], $c->d);
empty($value);
eval('return 1;');
exit;
exit(0);
die('goodbye');
$status = $broken ? exit(1) : 'ok';
