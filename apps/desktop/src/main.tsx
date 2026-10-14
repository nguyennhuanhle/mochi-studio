import React from "react";
import ReactDOM from "react-dom/client";
import { HashRouter, Route, Routes } from "react-router";
import "./i18n";
import "./styles.css";
import MainShell from "./components/MainShell";
import Countdown from "./routes/Countdown";
import Editor from "./routes/Editor";
import Home from "./routes/Home";
import Record from "./routes/Record";
import Recorded from "./routes/Recorded";
import RecorderBar from "./routes/RecorderBar";
import RegionPicker from "./routes/RegionPicker";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <HashRouter>
      <Routes>
        {/* Cửa sổ phụ khi quay (trong suốt, không có vỏ cửa sổ chính) */}
        <Route path="/recorder-bar" element={<RecorderBar />} />
        <Route path="/countdown" element={<Countdown />} />
        <Route path="/region-picker" element={<RegionPicker />} />
        {/* Cửa sổ chính */}
        <Route path="/" element={<MainShell><Home /></MainShell>} />
        <Route path="/record" element={<MainShell><Record /></MainShell>} />
        <Route path="/recorded" element={<MainShell><Recorded /></MainShell>} />
        <Route path="/editor" element={<MainShell><Editor /></MainShell>} />
      </Routes>
    </HashRouter>
  </React.StrictMode>,
);
