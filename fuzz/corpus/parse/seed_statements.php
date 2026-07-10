<?php
declare(strict_types=1);

function process(array $jobs, ?callable $notify = null): int
{
    $done = 0;
    foreach ($jobs as $id => $job) {
        switch (true) {
            case $job === null:
                continue 2;
            default:
                break;
        }
        try {
            if ($job->run()) {
                $done++;
            } else {
                throw new RuntimeException('failed');
            }
        } catch (RuntimeException $error) {
            if ($notify !== null) {
                $notify($id, $error);
            }
        } finally {
            unset($jobs[$id]);
        }
    }
    do {
        $done--;
    } while ($done > 100);
    for ($i = 0; $i < 2; $i++) {
        echo $i;
    }
    return $done;
}
