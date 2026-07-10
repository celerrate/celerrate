<?php

try {
    risky();
} catch (LogicException | \RuntimeException $error) {
    report($error);
} catch (Throwable) {
    recover();
} finally {
    cleanup();
}

try {
    once();
} finally {
    always();
}
