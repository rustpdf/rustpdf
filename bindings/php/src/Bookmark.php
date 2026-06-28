<?php

declare(strict_types=1);

namespace RustPdf;

/**
 * A document outline (bookmark) entry. Nest children with {@see child()} to
 * build a tree, then pass the root to {@see Document::addBookmark()}.
 */
final class Bookmark
{
    /** @var list<Bookmark> */
    public array $children;

    /**
     * @param float|null $top vertical position on the page (PDF user space), or
     *                        null to leave the viewer's default
     * @param list<Bookmark> $children
     */
    public function __construct(
        public string $title,
        public int $page,
        public ?float $top = null,
        array $children = [],
    ) {
        $this->children = array_values($children);
    }

    /** Append a child and return $this for chaining. */
    public function child(Bookmark $bookmark): self
    {
        $this->children[] = $bookmark;
        return $this;
    }

    /**
     * Pre-order flatten into a parallel list of
     * `[level, title, page, top]` tuples (level 0 = this node).
     *
     * @param list<array{0: int, 1: string, 2: int, 3: float|null}> $out
     */
    public function flatten(int $level, array &$out): void
    {
        $out[] = [$level, $this->title, $this->page, $this->top];
        foreach ($this->children as $c) {
            $c->flatten($level + 1, $out);
        }
    }
}
