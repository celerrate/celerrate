<?php

$simple = 'single \' quoted';
$double = "double $interpolated \"escaped\" ${deprecated} {$complex->call()}";
$offsets = "$array[0] $array[key] $array[$variable] $object->property";
$heredoc = <<<TEXT
    Indented $body text
    TEXT;
$nowdoc = <<<'RAW'
No $interpolation here
RAW;
$shell = `ls -la $directory`;
$binary = b"bytes";
