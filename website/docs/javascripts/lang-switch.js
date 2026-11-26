// Preserve current page path when switching between Chinese (/) and English (/en/).
(function () {
  function rewriteLinks() {
    var links = document.querySelectorAll(".md-select__link[hreflang]");
    if (!links.length) return;

    var path = window.location.pathname;
    var suffix = window.location.search + window.location.hash;
    // Detect base prefix (e.g. "/xcpc-workstation" or "") and whether we are under "/en/".
    var enMatch = path.match(/^(.*?)\/en(\/.*)?$/);
    var isEn = Boolean(enMatch);

    links.forEach(function (a) {
      var lang = a.getAttribute("hreflang");
      if (isEn) {
        var base = enMatch[1];
        var rest = enMatch[2] || "/";
        if (lang === "zh") {
          a.setAttribute("href", base + rest + suffix);
        } else if (lang === "en") {
          a.setAttribute("href", path + suffix);
        }
      } else {
        // Infer base prefix from the default Chinese alternate link if configured (e.g. "/xcpc-workstation/").
        var zhLink = document.querySelector('.md-select__link[hreflang="zh"]');
        var base = "";
        if (zhLink) {
          var rawZh = zhLink.getAttribute("href") || "/";
          if (rawZh.startsWith("/") && rawZh !== "/" && path.startsWith(rawZh)) {
            base = rawZh.replace(/\/$/, "");
          }
        }
        var rest = path.slice(base.length) || "/";
        if (!rest.startsWith("/")) rest = "/" + rest;
        if (lang === "en") {
          a.setAttribute("href", base + "/en" + rest + suffix);
        } else if (lang === "zh") {
          a.setAttribute("href", path + suffix);
        }
      }
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", rewriteLinks);
  } else {
    rewriteLinks();
  }
  if (typeof document$ !== "undefined" && document$.subscribe) {
    document$.subscribe(rewriteLinks);
  }
})();
