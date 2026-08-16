import { I18N, LANGS } from "./i18n.js";

const STORAGE_KEY = "rundog-lang";
const FONTS = {
  zh: "https://fonts.googleapis.com/css2?family=Noto+Sans+SC:wght@400;600;700&display=swap",
  ko: "https://fonts.googleapis.com/css2?family=Noto+Sans+KR:wght@400;600;700&display=swap",
  vi: "https://fonts.googleapis.com/css2?family=Be+Vietnam+Pro:wght@400;600;700&display=swap",
};

function detectLang() {
  const query = new URLSearchParams(location.search).get("lang");
  if (query && I18N[query]) {
    return query;
  }
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored && I18N[stored]) {
      return stored;
    }
  } catch {
    /* ignore */
  }
  const nav = (navigator.language || "ja").slice(0, 2).toLowerCase();
  return I18N[nav] ? nav : "ja";
}

function hrefFor(lang) {
  const path = location.pathname.endsWith("privacy.html") ? "./privacy.html" : "./";
  return lang === "ja" ? path : `${path}?lang=${lang}`;
}

function renderFeatures(items) {
  const root = document.getElementById("features");
  if (!root) {
    return;
  }
  root.replaceChildren(
    ...items.map((item) => {
      const card = document.createElement("article");
      card.className = "card";
      const title = document.createElement("h3");
      title.textContent = item.title;
      const body = document.createElement("p");
      body.textContent = item.body;
      card.append(title, body);
      return card;
    }),
  );
}

function renderMetrics(items) {
  const root = document.getElementById("metrics");
  if (!root) {
    return;
  }
  root.replaceChildren(
    ...items.map((text) => {
      const li = document.createElement("li");
      li.textContent = text;
      return li;
    }),
  );
}

function renderFaq(items) {
  const root = document.getElementById("faq");
  if (!root) {
    return;
  }
  root.replaceChildren(
    ...items.map((item) => {
      const details = document.createElement("details");
      const summary = document.createElement("summary");
      summary.textContent = item.q;
      const body = document.createElement("p");
      body.textContent = item.a;
      details.append(summary, body);
      return details;
    }),
  );
}

function renderPrivacy(paragraphs) {
  const root = document.getElementById("privacy-body");
  if (!root) {
    return;
  }
  root.replaceChildren(
    ...paragraphs.map((text) => {
      const p = document.createElement("p");
      p.textContent = text;
      return p;
    }),
  );
}

function renderLangs(lang) {
  const root = document.getElementById("langs");
  if (!root) {
    return;
  }
  root.replaceChildren(
    ...LANGS.map((item) => {
      const a = document.createElement("a");
      a.href = hrefFor(item.id);
      a.textContent = item.label;
      a.hreflang = item.id;
      if (item.id === lang) {
        a.setAttribute("aria-current", "true");
      }
      a.addEventListener("click", (event) => {
        event.preventDefault();
        apply(item.id, true);
      });
      return a;
    }),
  );
}

function loadFont(lang) {
  const href = FONTS[lang];
  if (!href) {
    return;
  }
  const id = `font-${lang}`;
  if (document.getElementById(id)) {
    return;
  }
  const link = document.createElement("link");
  link.id = id;
  link.rel = "stylesheet";
  link.href = href;
  document.head.append(link);
}

function apply(lang, pushUrl) {
  const t = I18N[lang] ?? I18N.ja;
  document.documentElement.lang = lang;
  document.title = t.title;
  const description = document.querySelector('meta[name="description"]');
  if (description) {
    description.setAttribute("content", t.description);
  }
  const og = document.querySelector('meta[property="og:description"]');
  if (og) {
    og.setAttribute("content", t.description);
  }

  document.querySelectorAll("[data-i18n]").forEach((el) => {
    const value = t[el.dataset.i18n];
    if (typeof value === "string") {
      el.textContent = value;
    }
  });
  document.querySelectorAll("[data-i18n-alt]").forEach((el) => {
    const value = t[el.dataset.i18nAlt];
    if (typeof value === "string") {
      el.setAttribute("alt", value);
    }
  });

  renderFeatures(t.features ?? []);
  renderMetrics(t.metrics ?? []);
  renderFaq(t.faq ?? []);
  renderPrivacy(t.privacyBody ?? []);
  renderLangs(lang);
  loadFont(lang);

  try {
    localStorage.setItem(STORAGE_KEY, lang);
  } catch {
    /* ignore */
  }

  if (pushUrl) {
    const url = hrefFor(lang);
    history.replaceState(null, "", url);
  }
}

const lang = detectLang();
apply(lang, false);
if (!new URLSearchParams(location.search).has("lang") && lang !== "ja") {
  history.replaceState(null, "", hrefFor(lang));
}
