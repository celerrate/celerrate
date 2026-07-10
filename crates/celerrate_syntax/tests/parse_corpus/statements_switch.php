<?php

switch ($signal) {
    case 'red':
    case 'amber':
        $action = 'stop';
        break;
    case 'green':
        $action = 'go';
        break;
    default:
        $action = 'wait';
}

switch ($tight) { ; case 1: echo 1; }

switch ($state):
    case 'on':
        echo 1;
        break;
    default:
        echo 0;
endswitch;
