<?php
"plain";
"a $name b";
"$user->name and $user?->name";
"$items[0] $map[key] $grid[$x] $list[-1]";
"x {$a->b(1)} y";
"{$f(['k' => 1])}";
"${legacy}";
`ls $directory`;
<<<TXT
Hello $name, total {$cart->total()}
TXT;
<<<'RAW'
No $interpolation here
RAW;
b"binary $x";
