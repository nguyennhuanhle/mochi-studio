import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import vi from "./vi.json";
import en from "./en.json";

const STORAGE_KEY = "mochi.lang";

/** Mặc định theo ngôn ngữ hệ điều hành; người dùng đổi thì nhớ lại. */
function initialLanguage(): "vi" | "en" {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "vi" || saved === "en") return saved;
  } catch {
    /* localStorage có thể không dùng được */
  }
  return navigator.language.toLowerCase().startsWith("vi") ? "vi" : "en";
}

i18n.use(initReactI18next).init({
  resources: { vi: { translation: vi }, en: { translation: en } },
  lng: initialLanguage(),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

i18n.on("languageChanged", (lng) => {
  try {
    localStorage.setItem(STORAGE_KEY, lng);
  } catch {
    /* bỏ qua */
  }
  document.documentElement.lang = lng;
});

export default i18n;
