package dev.rustpdf;

import java.util.ArrayList;
import java.util.List;

/**
 * A document outline (bookmark) entry. Nest children with {@link #child} to
 * build a tree, then pass the root to {@link Document#addBookmark(Bookmark)}.
 *
 * <pre>{@code
 * Bookmark root = new Bookmark("Chapter 1", 0)
 *         .child(new Bookmark("Section 1.1", 1));
 * doc.addBookmark(root);
 * }</pre>
 */
public final class Bookmark {
    final String title;
    final int page;
    final Double top; // null = no explicit vertical position
    final List<Bookmark> children = new ArrayList<>();

    /** A bookmark targeting {@code page} (0-based), positioned at the page top. */
    public Bookmark(String title, int page) {
        this(title, page, null);
    }

    /**
     * A bookmark targeting {@code page} (0-based). {@code top} is the optional
     * y-coordinate to scroll to (PDF user space); {@code null} scrolls to the
     * page top.
     */
    public Bookmark(String title, int page, Double top) {
        this.title = title;
        this.page = page;
        this.top = top;
    }

    /** Append {@code child} under this entry; returns {@code this} for chaining. */
    public Bookmark child(Bookmark child) {
        children.add(child);
        return this;
    }

    /** Pre-order flatten into the given accumulator (level 0 = this root). */
    void flatten(int level, List<Bookmark> out, List<Integer> levels) {
        out.add(this);
        levels.add(level);
        for (Bookmark c : children) {
            c.flatten(level + 1, out, levels);
        }
    }
}
