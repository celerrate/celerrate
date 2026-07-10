<!DOCTYPE html>
<body>
<?php

declare(strict_types=1);

$greeting = 'Hello';
echo "$greeting, {$_SERVER['REMOTE_ADDR']} !";

?>
</body>
