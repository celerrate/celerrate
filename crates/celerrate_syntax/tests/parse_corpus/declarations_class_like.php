<?php

abstract class Base extends Root implements Countable, Stringable {}

final readonly class Value {}

interface Shape extends HasArea, HasPerimeter {}

trait Greets {}

class List {}

$instance = new class(1) extends Base {
    public int $inline = 0;
};

$flag = new readonly class {};

readonly($flag);
