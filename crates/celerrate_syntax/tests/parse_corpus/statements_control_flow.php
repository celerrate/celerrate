<?php

if ($ready) {
    echo 'go';
} elseif ($waiting) {
    echo 'hold';
} else {
    echo 'stop';
}

if ($a) if ($b) echo 1; else echo 2;

$i = 0;
while ($i < 3) {
    $i++;
}

do {
    $i--;
} while ($i > 0);

for ($j = 0, $k = 9; $j < $k; $j++, $k--) {
    echo $j;
}

foreach ($items as $item) {
    echo $item;
}

foreach ($map as $key => [$first, &$second]) {
    echo $key;
}

foreach ($queue as &$task) {
    $task = null;
}
