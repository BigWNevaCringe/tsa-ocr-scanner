import React, { useEffect, useMemo, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import "./styles.css";

type AppConfig = {
  watchFolder: string;
  apiUrl: string;
  scanKey: string;
  keyName: string;
};

type WatcherLog = {
  level: "info" | "success" | "error";
  message: string;
  path?: string;
};

const defaultConfig: AppConfig = {
  watchFolder: "",
  apiUrl: "https://tsa-ocr.lhu.edu.vn/api/v1/scan-machines/profile-image",
  scanKey: "",
  keyName: "ocr",
};

function App() {
  const [config, setConfig] = useState<AppConfig>(defaultConfig);
  const [running, setRunning] = useState(false);
  const [saving, setSaving] = useState(false);
  const [logs, setLogs] = useState<WatcherLog[]>([]);

  const canStart = useMemo(() => {
    return Boolean(config.watchFolder && config.apiUrl && config.scanKey && config.keyName);
  }, [config]);

  useEffect(() => {
    invoke<AppConfig>("load_config")
      .then((saved) => setConfig({ ...defaultConfig, ...saved }))
      .catch((error) => pushLog("error", String(error)));

    invoke<boolean>("watcher_status")
      .then(setRunning)
      .catch(() => setRunning(false));

    const unlisten = listen<WatcherLog>("watcher-log", (event) => {
      setLogs((current) => [event.payload, ...current].slice(0, 120));
    });

    return () => {
      unlisten.then((stop) => stop()).catch(() => undefined);
    };
  }, []);

  function updateConfig<K extends keyof AppConfig>(key: K, value: AppConfig[K]) {
    setConfig((current) => ({ ...current, [key]: value }));
  }

  function pushLog(level: WatcherLog["level"], message: string, path?: string) {
    setLogs((current) => [{ level, message, path }, ...current].slice(0, 120));
  }

  async function chooseFolder() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Chọn thư mục output máy scan",
    });

    if (typeof selected === "string") {
      updateConfig("watchFolder", selected);
    }
  }

  async function saveConfig() {
    setSaving(true);
    try {
      await invoke("save_config", { config });
      pushLog("success", "Đã lưu cấu hình");
    } catch (error) {
      pushLog("error", String(error));
    } finally {
      setSaving(false);
    }
  }

  async function startWatcher() {
    try {
      await invoke("start_watcher", { config });
      setRunning(true);
      pushLog("success", "Watcher đang chạy");
    } catch (error) {
      pushLog("error", String(error));
    }
  }

  async function stopWatcher() {
    try {
      await invoke("stop_watcher");
      setRunning(false);
      pushLog("info", "Watcher đã dừng");
    } catch (error) {
      pushLog("error", String(error));
    }
  }

  return (
    <main className="app-shell">
      <section className="header">
        <div>
          <p className="eyebrow">LH TSA OCR</p>
          <h1>Scan Watcher</h1>
        </div>
        <span className={running ? "status status-on" : "status"}>{running ? "Running" : "Stopped"}</span>
      </section>

      <section className="panel">
        <div className="field">
          <label>Thư mục output máy scan</label>
          <div className="folder-row">
            <input value={config.watchFolder} onChange={(event) => updateConfig("watchFolder", event.target.value)} />
            <button type="button" onClick={chooseFolder}>
              Chọn
            </button>
          </div>
        </div>

        <div className="field">
          <label>API URL</label>
          <input value={config.apiUrl} onChange={(event) => updateConfig("apiUrl", event.target.value)} />
        </div>

        <div className="grid">
          <div className="field">
            <label>Scan key</label>
            <input
              type="password"
              value={config.scanKey}
              onChange={(event) => updateConfig("scanKey", event.target.value)}
              placeholder="skm_xxx"
            />
          </div>

          <div className="field">
            <label>Key name</label>
            <input value={config.keyName} onChange={(event) => updateConfig("keyName", event.target.value)} />
          </div>
        </div>

        <div className="actions">
          <button type="button" className="secondary" onClick={saveConfig} disabled={saving}>
            {saving ? "Đang lưu" : "Lưu cấu hình"}
          </button>
          {running ? (
            <button type="button" className="danger" onClick={stopWatcher}>
              Stop
            </button>
          ) : (
            <button type="button" onClick={startWatcher} disabled={!canStart}>
              Start watcher
            </button>
          )}
        </div>
      </section>

      <section className="panel log-panel">
        <div className="log-header">
          <h2>Log upload</h2>
          <button type="button" className="ghost" onClick={() => setLogs([])}>
            Clear
          </button>
        </div>

        <div className="logs">
          {logs.length === 0 ? (
            <p className="muted">Chưa có log.</p>
          ) : (
            logs.map((log, index) => (
              <div className={`log log-${log.level}`} key={`${log.message}-${index}`}>
                <span>{log.level}</span>
                <p>{log.message}</p>
                {log.path ? <small>{log.path}</small> : null}
              </div>
            ))
          )}
        </div>
      </section>
    </main>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
