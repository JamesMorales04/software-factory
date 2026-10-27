import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";

export type Theme = "system" | "light" | "dark";
export type Density = "comfortable" | "compact";

const THEME_KEY = "factory-office-theme";
const DENSITY_KEY = "factory-office-density";

interface ThemeContextValue {
  theme: Theme;
  density: Density;
  setTheme: (theme: Theme) => void;
  setDensity: (density: Density) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

function readTheme(): Theme {
  const value = localStorage.getItem(THEME_KEY);
  return value === "light" || value === "dark" || value === "system"
    ? value
    : "system";
}

function readDensity(): Density {
  return localStorage.getItem(DENSITY_KEY) === "compact"
    ? "compact"
    : "comfortable";
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(readTheme);
  const [density, setDensityState] = useState<Density>(readDensity);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem(THEME_KEY, theme);
  }, [theme]);

  useEffect(() => {
    document.documentElement.dataset.density = density;
    localStorage.setItem(DENSITY_KEY, density);
  }, [density]);

  const value = useMemo<ThemeContextValue>(
    () => ({
      theme,
      density,
      setTheme: setThemeState,
      setDensity: setDensityState,
    }),
    [theme, density],
  );

  return (
    <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
  );
}

export function useTheme(): ThemeContextValue {
  const value = useContext(ThemeContext);
  if (!value) {
    throw new Error("useTheme must be used inside ThemeProvider");
  }
  return value;
}
