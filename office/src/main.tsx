import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { HashRouter } from "react-router-dom";
import "@fontsource-variable/ibm-plex-sans";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import { App } from "./App";
import { ThemeProvider } from "./theme";
import "./styles.css";

const root = document.getElementById("root");
if (!root) {
  throw new Error("missing #root element");
}
createRoot(root).render(
  <StrictMode>
    <ThemeProvider>
      <HashRouter>
        <App />
      </HashRouter>
    </ThemeProvider>
  </StrictMode>,
);
