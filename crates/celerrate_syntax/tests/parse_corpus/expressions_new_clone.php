<?php
new Foo;
new Foo(1, 2);
new \Fully\Qualified($x);
new static;
new $class;
new $factory->product(1);
new ($resolver->pick())($x);
new Foo()->bar()->baz;
clone $entity;
clone $entity->child;
(clone $prototype)->mutate();
clone($entity, ['id' => null]);
clone($entity)->touch();
