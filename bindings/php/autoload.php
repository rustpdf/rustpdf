<?php

declare(strict_types=1);

// Minimal PSR-4 autoloader for running without Composer.
spl_autoload_register(static function (string $class): void {
    $prefix = 'RustPdf\\';
    if (!str_starts_with($class, $prefix)) {
        return;
    }
    $relative = substr($class, strlen($prefix));
    $file = __DIR__ . '/src/' . str_replace('\\', '/', $relative) . '.php';
    if (is_file($file)) {
        require $file;
    }
});
