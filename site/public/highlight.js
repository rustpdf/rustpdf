// Tiny self-hosted syntax highlighter (no external dependency, no network).
// Tokenizes comments, strings, numbers, keywords, types and function calls —
// generic enough to read well across Rust, Python, JS/TS, shell, Java, etc.
(function () {
  var KW = new Set(
    ("let mut fn use pub const static return if else match for while loop impl " +
     "struct enum trait where move ref dyn as crate self Self super true false None " +
     "import from with def class lambda try except raise finally yield await async " +
     "var function new public private protected void int double float boolean byte " +
     "string interface extends implements throws throw package func go defer chan map " +
     "require echo export local nil null undefined and or not in is del pass with")
      .split(" ")
  );

  // Order matters: comment, then string, then number, then identifier.
  var RE = /((?:(?<![:\w])\/\/[^\n]*)|(?:(?<!\w)#[^\n]*))|("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|`(?:[^`\\]|\\.)*`)|(\b\d[\d_]*\.?\d*\b)|([A-Za-z_$][\w$]*)/g;

  function esc(s) {
    return s.replace(/[&<>]/g, function (c) {
      return c === "&" ? "&amp;" : c === "<" ? "&lt;" : "&gt;";
    });
  }

  function highlight(code) {
    var out = "";
    var last = 0;
    var m;
    RE.lastIndex = 0;
    while ((m = RE.exec(code)) !== null) {
      out += esc(code.slice(last, m.index));
      last = RE.lastIndex;
      if (m[1]) {
        out += '<span class="tok-c">' + esc(m[1]) + "</span>";
      } else if (m[2]) {
        out += '<span class="tok-s">' + esc(m[2]) + "</span>";
      } else if (m[3]) {
        out += '<span class="tok-n">' + esc(m[3]) + "</span>";
      } else if (m[4]) {
        var w = m[4];
        var cls = null;
        if (KW.has(w)) cls = "tok-k";
        else if (/^[A-Z]/.test(w)) cls = "tok-t";
        else if (code[RE.lastIndex] === "(") cls = "tok-f";
        out += cls ? '<span class="' + cls + '">' + esc(w) + "</span>" : esc(w);
      }
    }
    out += esc(code.slice(last));
    return out;
  }

  function run() {
    document.querySelectorAll("pre code").forEach(function (el) {
      if (el.dataset.hl) return;
      el.innerHTML = highlight(el.textContent);
      el.dataset.hl = "1";
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", run);
  } else {
    run();
  }
})();
