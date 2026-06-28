// Mobile nav toggle — docs pages load docs.js (not app.js), so the same
// injected hamburger lives here. Hidden ≥820px via CSS.
(() => {
  const inner = document.querySelector(".nav .nav-inner");
  const links = inner && inner.querySelector(".nav-links");
  if (!inner || !links || inner.querySelector(".nav-toggle")) return;
  links.id = links.id || "nav-links";
  const btn = document.createElement("button");
  btn.className = "nav-toggle";
  btn.type = "button";
  btn.setAttribute("aria-label", "Menu");
  btn.setAttribute("aria-controls", links.id);
  btn.setAttribute("aria-expanded", "false");
  btn.innerHTML = '<span class="bars" aria-hidden="true"></span>';
  links.insertAdjacentElement("beforebegin", btn);
  const set = (open) => {
    btn.setAttribute("aria-expanded", String(open));
    links.classList.toggle("open", open);
  };
  btn.addEventListener("click", () => set(btn.getAttribute("aria-expanded") !== "true"));
  links.addEventListener("click", (e) => { if (e.target.closest("a")) set(false); });
  document.addEventListener("keydown", (e) => { if (e.key === "Escape") set(false); });
  document.addEventListener("click", (e) => {
    if (links.classList.contains("open") && !inner.contains(e.target)) set(false);
  });
})();

// Copy buttons on every .code block
document.querySelectorAll(".code").forEach((block) => {
  const pre = block.querySelector("pre");
  if (!pre) return;
  const btn = document.createElement("button");
  btn.className = "copy";
  btn.textContent = "Copy";
  btn.addEventListener("click", () => {
    navigator.clipboard.writeText(pre.innerText);
    btn.textContent = "Copied ✓";
    setTimeout(() => (btn.textContent = "Copy"), 1400);
  });
  block.appendChild(btn);
});

// Scrollspy — highlight the current section in the left nav and right TOC
const headings = [...document.querySelectorAll(".docs-main h2[id], .docs-main h3[id]")];
const links = new Map();
document.querySelectorAll(".docs-side a[href^='#'], .docs-toc a[href^='#']").forEach((a) => {
  const id = a.getAttribute("href").slice(1);
  if (!links.has(id)) links.set(id, []);
  links.get(id).push(a);
});

if (headings.length && "IntersectionObserver" in window) {
  let current = null;
  const obs = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (e.isIntersecting) current = e.target.id;
      }
      links.forEach((els) => els.forEach((el) => el.classList.remove("active")));
      if (current && links.has(current)) links.get(current).forEach((el) => el.classList.add("active"));
    },
    { rootMargin: "-70px 0px -75% 0px", threshold: 0 }
  );
  headings.forEach((h) => obs.observe(h));
}
