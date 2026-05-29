import { useState, useEffect } from "react";

const STORAGE_KEY = "gnome-remote-theme";

export function useTheme() {
  const [theme, setTheme] = useState<"light" | "dark">("light");

  useEffect(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      console.log("[DEBUG] useTheme - loading from localStorage:", stored);
      if (stored === "light" || stored === "dark") {
        setTheme(stored);
      }
    } catch (e) {
      console.warn("[ThemeManager] Failed to load theme:", e);
    }
  }, []);

  useEffect(() => {
    console.log("[DEBUG] useTheme - theme changed to:", theme);
    console.log("[DEBUG] useTheme - document.documentElement before:", document.documentElement.getAttribute("data-theme"));
    document.documentElement.setAttribute("data-theme", theme);
    console.log("[DEBUG] useTheme - document.documentElement after:", document.documentElement.getAttribute("data-theme"));
    try {
      localStorage.setItem(STORAGE_KEY, theme);
      console.log("[DEBUG] useTheme - saved to localStorage:", localStorage.getItem(STORAGE_KEY));
    } catch (e) {
      console.warn("[ThemeManager] Failed to save theme:", e);
    }
  }, [theme]);

  return { theme, setTheme };
}