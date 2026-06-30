<?php

declare(strict_types=1);

namespace RustPdf;

/**
 * Resolves and (if needed) downloads the prebuilt native `libpdf_ffi` for the
 * current platform.
 *
 * Why a downloader at all: the native library is large and platform-specific,
 * and Packagist (unlike npm/PyPI) only serves the git tree — it cannot host
 * per-platform binaries. So each release attaches the cdylibs to a GitHub
 * Release and this class fetches the one matching the host into the package's
 * own `lib/<os>-<arch>/` directory, where {@see Ffi::libPath()} then finds it.
 *
 * Composer only fires `post-install-cmd`/`post-update-cmd` for the *root*
 * package, never for a dependency, so we cannot rely on a script firing on
 * `composer require`. Two entry points cover every case:
 *   - {@see ensure()} is called lazily by Ffi on first use (zero config), and
 *   - `bin/rustpdf-install-lib` runs it explicitly at deploy/CI time (so the
 *     download happens once, ahead of any read-only production filesystem).
 *
 * Set `RUSTPDF_LIB` to an absolute path to bypass all of this, or
 * `RUSTPDF_NO_DOWNLOAD=1` to forbid the network fetch (build locally instead).
 */
final class Installer
{
    /**
     * Native-library version. The GitHub Release tag is `php-v<VERSION>` and
     * the attached asset names embed it. Kept in lockstep with the package
     * version in composer.json and bumped by the release pipeline.
     */
    public const VERSION = '0.4.4';

    /**
     * Public GitHub repo that hosts the per-platform release assets. This is the
     * Packagist mirror, NOT the (private) monorepo — assets on a private repo's
     * release 404 for unauthenticated clients. The binary release shares the
     * source tag, `v<VERSION>`.
     */
    private const REPO = 'rustpdf/rustpdf-php';

    /** Directory the platform cdylib is installed into (package_root/lib/<os>-<arch>). */
    public static function libDir(): string
    {
        return \dirname(__DIR__) . '/lib/' . self::platformKey();
    }

    /**
     * `<os>-<arch>` slug for the host, matching the release asset layout.
     * Throws on a platform we don't ship a prebuilt binary for.
     */
    public static function platformKey(): string
    {
        $arch = self::normalizeArch(php_uname('m'));
        $os = match (PHP_OS_FAMILY) {
            'Darwin' => 'darwin',
            'Windows' => 'windows',
            'Linux' => 'linux',
            default => null,
        };
        if ($os === null || $arch === null) {
            throw new PdfException(
                'unsupported platform ' . PHP_OS_FAMILY . '/' . php_uname('m')
                . '; build libpdf_ffi yourself and point RUSTPDF_LIB at it'
            );
        }
        return "$os-$arch";
    }

    /** Normalize the many spellings of x86_64 / arm64 to our two arch slugs. */
    private static function normalizeArch(string $m): ?string
    {
        $m = strtolower($m);
        return match (true) {
            in_array($m, ['x86_64', 'amd64', 'x64'], true) => 'x86_64',
            in_array($m, ['arm64', 'aarch64'], true) => 'aarch64',
            default => null,
        };
    }

    /** Map platformKey() -> the cdylib file name carried in the package. */
    private static function platformKeyToOs(string $key): string
    {
        return explode('-', $key, 2)[0];
    }

    /**
     * Ensure the native library exists for the host and return its absolute
     * path, downloading the prebuilt binary if it isn't present yet.
     *
     * @param bool $verbose echo progress (used by the CLI installer)
     */
    public static function ensure(bool $verbose = false): string
    {
        $env = getenv('RUSTPDF_LIB');
        if ($env !== false && $env !== '' && is_file($env)) {
            return $env;
        }

        $key = self::platformKey();
        $dir = self::libDir();
        $dest = $dir . '/' . Ffi::libFileName();
        if (is_file($dest)) {
            if ($verbose) {
                self::out("libpdf_ffi already present for $key at $dest");
            }
            return $dest;
        }

        if (getenv('RUSTPDF_NO_DOWNLOAD')) {
            throw new PdfException(
                "libpdf_ffi not found for $key and RUSTPDF_NO_DOWNLOAD is set; "
                . 'build it with `cargo build -p pdf-ffi --release` or set RUSTPDF_LIB'
            );
        }

        $asset = self::assetName($key);
        $url = sprintf(
            'https://github.com/%s/releases/download/v%s/%s',
            self::REPO,
            self::VERSION,
            $asset
        );

        if ($verbose) {
            self::out("downloading $asset ...");
        }
        if (!is_dir($dir) && !@mkdir($dir, 0o755, true) && !is_dir($dir)) {
            throw new PdfException("could not create $dir (read-only filesystem?); "
                . 'run `php vendor/rust-pdf/rustpdf/bin/rustpdf-install-lib` at deploy time '
                . 'or set RUSTPDF_LIB');
        }

        $data = self::httpGet($url);
        if ($data === null || $data === '') {
            throw new PdfException("failed to download $url; set RUSTPDF_LIB to a "
                . 'locally built libpdf_ffi, or build it with `cargo build -p pdf-ffi --release`');
        }

        // Atomic write so a partial download never looks installed.
        $tmp = $dest . '.tmp.' . getmypid();
        if (file_put_contents($tmp, $data) === false) {
            throw new PdfException("could not write $tmp");
        }
        if (PHP_OS_FAMILY !== 'Windows') {
            @chmod($tmp, 0o755);
        }
        if (!rename($tmp, $dest)) {
            @unlink($tmp);
            throw new PdfException("could not move downloaded lib into $dest");
        }

        if ($verbose) {
            self::out('installed -> ' . $dest . ' (' . number_format(\strlen($data)) . ' bytes)');
        }
        return $dest;
    }

    /** Release asset name for a platform key (embeds os/arch, real extension). */
    private static function assetName(string $key): string
    {
        $os = self::platformKeyToOs($key);
        // Windows ships pdf_ffi.dll; the others libpdf_ffi.{so,dylib}.
        return match ($os) {
            'windows' => "pdf_ffi-$key.dll",
            'darwin' => "libpdf_ffi-$key.dylib",
            default => "libpdf_ffi-$key.so",
        };
    }

    /** Minimal HTTPS GET that follows redirects, via curl ext or streams. */
    private static function httpGet(string $url): ?string
    {
        if (\function_exists('curl_init')) {
            $ch = curl_init($url);
            curl_setopt_array($ch, [
                CURLOPT_RETURNTRANSFER => true,
                CURLOPT_FOLLOWLOCATION => true,
                CURLOPT_FAILONERROR => true,
                CURLOPT_CONNECTTIMEOUT => 30,
                CURLOPT_TIMEOUT => 600,
                CURLOPT_USERAGENT => 'rustpdf-php-installer',
            ]);
            $body = curl_exec($ch);
            $err = curl_error($ch);
            if (PHP_VERSION_ID < 80000) {
                curl_close($ch); // no-op since 8.0, deprecated 8.5+
            }
            if ($body === false || $body === '') {
                if ($err !== '') {
                    throw new PdfException("download error: $err");
                }
                return null;
            }
            return (string) $body;
        }

        // Fallback: stream wrapper (needs allow_url_fopen).
        $ctx = stream_context_create(['http' => [
            'follow_location' => 1,
            'timeout' => 600,
            'user_agent' => 'rustpdf-php-installer',
        ]]);
        $body = @file_get_contents($url, false, $ctx);
        return $body === false ? null : $body;
    }

    private static function out(string $msg): void
    {
        fwrite(STDERR, "[rustpdf] $msg\n");
    }
}
