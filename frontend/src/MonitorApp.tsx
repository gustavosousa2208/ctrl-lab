import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/*
 * Live MCU monitor: what a board is doing, while it does it.
 *
 * Deliberately NOT the simulation scope. That scope plots a model; this plots a
 * board, and the two agreeing is the result stage E exists to measure - merging
 * them would make the most interesting number in the project invisible.
 *
 * Read-only. Nothing here can arm, load or steer a board.
 */

/** Samples kept on screen. At the 50 ms fixtures' 20 Hz this is 30 s. */
const WINDOW_SAMPLES = 600;

/* Distinguishable on the dark screen below, and stable per signal index so a
 * trace does not change colour between runs. */
const TRACE_COLOURS = [
  "#ffb454", "#7fd4a0", "#6fb7ff", "#ff8fa3",
  "#d6c76a", "#b79bff", "#71d6d6", "#ff9f6f",
];

type MonitorRow = { t: number; values: number[] };
type MonitorStatus = {
  running: boolean;
  rows: number;
  damaged: number;
  error: string | null;
};

function formatValue(value: number) {
  if (!Number.isFinite(value)) {
    return String(value);
  }
  if (value !== 0 && Math.abs(value) < 1e-3) {
    return value.toExponential(2);
  }
  return value.toFixed(4);
}

/** One polyline per signal, scaled over the whole window so the traces share
 *  a vertical axis and can be read against each other. */
function buildTracePaths(samples: MonitorRow[], width: number, height: number) {
  if (samples.length < 2) {
    return { paths: [] as string[], min: 0, max: 0 };
  }

  const signalCount = samples[0].values.length;
  let min = Infinity;
  let max = -Infinity;
  for (const sample of samples) {
    for (const value of sample.values) {
      if (!Number.isFinite(value)) continue;
      if (value < min) min = value;
      if (value > max) max = value;
    }
  }
  if (!Number.isFinite(min) || !Number.isFinite(max)) {
    return { paths: [], min: 0, max: 0 };
  }

  // A flat trace would divide by zero and, worse, would jump to the middle of
  // the screen on the first sample that differs. Pad it instead.
  if (max - min < 1e-9) {
    min -= 0.5;
    max += 0.5;
  }
  const span = max - min;
  const lastIndex = samples.length - 1;

  const paths: string[] = [];
  for (let signal = 0; signal < signalCount; signal++) {
    let path = "";
    for (let index = 0; index < samples.length; index++) {
      const value = samples[index].values[signal];
      if (!Number.isFinite(value)) continue;
      const x = (index / lastIndex) * width;
      const y = height - ((value - min) / span) * height;
      path += `${path === "" ? "M" : "L"} ${x.toFixed(2)} ${y.toFixed(2)} `;
    }
    paths.push(path.trim());
  }
  return { paths, min, max };
}

export default function MonitorApp() {
  const [ports, setPorts] = useState<string[]>([]);
  const [port, setPort] = useState("");
  const [status, setStatus] = useState<MonitorStatus>({
    running: false,
    rows: 0,
    damaged: 0,
    error: null,
  });
  const [startError, setStartError] = useState<string | null>(null);

  // The samples live in a ref and the render is driven by a counter, so a burst
  // of rows costs one render rather than one per row.
  const samplesRef = useRef<MonitorRow[]>([]);
  const [revision, setRevision] = useState(0);

  const refreshPorts = useCallback(async () => {
    try {
      const found = await invoke<string[]>("monitor_ports");
      setPorts(found);
      setPort((current) => (current === "" ? found[0] ?? "" : current));
    } catch (error) {
      setStartError(String(error));
    }
  }, []);

  useEffect(() => {
    void refreshPorts();
  }, [refreshPorts]);

  useEffect(() => {
    const subscriptions = [
      listen<MonitorRow>("monitor://row", (event) => {
        const samples = samplesRef.current;
        // A plan with a different signal count is a different run, not more of
        // this one. Starting over beats splicing two shapes together.
        if (samples.length > 0 && samples[0].values.length !== event.payload.values.length) {
          samplesRef.current = [];
        }
        samplesRef.current.push(event.payload);
        if (samplesRef.current.length > WINDOW_SAMPLES) {
          samplesRef.current.splice(0, samplesRef.current.length - WINDOW_SAMPLES);
        }
        setRevision((value) => value + 1);
      }),
      listen<MonitorStatus>("monitor://status", (event) => setStatus(event.payload)),
    ];

    return () => {
      for (const subscription of subscriptions) {
        void subscription.then((unlisten) => unlisten());
      }
    };
  }, []);

  const start = useCallback(async () => {
    setStartError(null);
    samplesRef.current = [];
    setRevision((value) => value + 1);
    try {
      await invoke("monitor_start", { port });
    } catch (error) {
      setStartError(String(error));
    }
  }, [port]);

  const stop = useCallback(async () => {
    await invoke("monitor_stop");
  }, []);

  const samples = samplesRef.current;
  const plot = useMemo(
    () => buildTracePaths(samples, 960, 320),
    // revision is the dependency: the array is mutated in place on purpose.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [revision],
  );
  const latest = samples[samples.length - 1];
  const elapsed = samples.length > 1 ? latest.t - samples[0].t : 0;

  return (
    <main className="monitor">
      <header className="monitor__bar">
        <span className="monitor__eyebrow">MCU MONITOR</span>

        <label className="monitor__field">
          port
          <select value={port} onChange={(event) => setPort(event.target.value)}
                  disabled={status.running}>
            {ports.length === 0 && <option value="">no ports found</option>}
            {ports.map((name) => (
              <option key={name} value={name}>{name}</option>
            ))}
          </select>
        </label>

        <button type="button" onClick={() => void refreshPorts()} disabled={status.running}>
          rescan
        </button>
        <button type="button" onClick={() => void start()}
                disabled={status.running || port === ""}>
          start
        </button>
        <button type="button" onClick={() => void stop()} disabled={!status.running}>
          stop
        </button>

        <span className={`monitor__lamp${status.running ? " monitor__lamp--live" : ""}`}>
          {status.running ? "reading" : "idle"}
        </span>
      </header>

      <section className="monitor__screen">
        <svg viewBox="0 0 960 320" preserveAspectRatio="none" role="img"
             aria-label="live signal traces from the board">
          {plot.paths.map((path, index) => (
            <path key={index} d={path} fill="none" strokeWidth={1.5}
                  stroke={TRACE_COLOURS[index % TRACE_COLOURS.length]} />
          ))}
        </svg>
        {samples.length < 2 && (
          <p className="monitor__hint">
            {status.running
              ? `listening on ${port} - nothing yet. The board only streams while a run is in progress.`
              : "pick a port and press start."}
          </p>
        )}
      </section>

      <footer className="monitor__readout">
        <dl>
          <div><dt>rows</dt><dd>{status.rows}</dd></div>
          {/* Shown always, not only when non-zero: a plot that hid dropped
              samples would be quietly lying about what the board did. */}
          <div className={status.damaged > 0 ? "monitor__damaged" : undefined}>
            <dt>dropped</dt><dd>{status.damaged}</dd>
          </div>
          <div><dt>window</dt><dd>{samples.length} / {WINDOW_SAMPLES}</dd></div>
          <div><dt>span</dt><dd>{elapsed.toFixed(2)} s</dd></div>
          <div><dt>range</dt>
            <dd>{samples.length > 1 ? `${formatValue(plot.min)} … ${formatValue(plot.max)}` : "—"}</dd>
          </div>
        </dl>

        {latest && (
          <ul className="monitor__legend">
            <li><span className="monitor__swatch monitor__swatch--t" />t<b>{latest.t.toFixed(3)}</b></li>
            {latest.values.map((value, index) => (
              <li key={index}>
                <span className="monitor__swatch"
                      style={{ background: TRACE_COLOURS[index % TRACE_COLOURS.length] }} />
                s{index}<b>{formatValue(value)}</b>
              </li>
            ))}
          </ul>
        )}

        {(startError ?? status.error) && (
          <p className="monitor__error">{startError ?? status.error}</p>
        )}
      </footer>
    </main>
  );
}
