import { useEffect, useRef, useState } from "react";
import { downloadDogfoodScreenshot, type DogfoodReport } from "../api";

/** One operator-requested transfer, owned by this mounted diagnostics view. */
export function useScreenshotDownload(operatorToken: string) {
  const active = useRef<AbortController | undefined>(undefined);
  const [downloadingReportId, setDownloadingReportId] = useState<string>();
  const [failure, setFailure] = useState<{ reportId: string; message: string }>();

  useEffect(() => {
    setDownloadingReportId(undefined);
    setFailure(undefined);
    return () => {
      active.current?.abort();
      active.current = undefined;
    };
  }, [operatorToken]);

  async function download(report: DogfoodReport) {
    if (!operatorToken || !report.attachment_name || active.current) return;
    const request = new AbortController();
    active.current = request;
    setDownloadingReportId(report.id);
    setFailure(undefined);
    const deadline = window.setTimeout(() => request.abort(new DOMException("Screenshot download timed out", "TimeoutError")), 30_000);
    request.signal.addEventListener("abort", () => window.clearTimeout(deadline), { once: true });
    try {
      const blob = await downloadDogfoodScreenshot(operatorToken, report.attachment_name, request.signal);
      if (request.signal.aborted || active.current !== request) return;
      const url = URL.createObjectURL(blob);
      try {
        const anchor = document.createElement("a");
        anchor.href = url;
        anchor.download = report.attachment_name;
        anchor.click();
      } finally {
        URL.revokeObjectURL(url);
      }
    } catch {
      if (active.current !== request) return;
      const timedOut = request.signal.reason instanceof DOMException && request.signal.reason.name === "TimeoutError";
      if (!request.signal.aborted || timedOut) {
        setFailure({ reportId: report.id, message: timedOut
          ? "Screenshot download timed out. Try again."
          : "Screenshot could not be downloaded. Check your connection and try again." });
      }
    } finally {
      window.clearTimeout(deadline);
      if (active.current === request) {
        active.current = undefined;
        setDownloadingReportId(undefined);
      }
    }
  }

  return { download, downloadingReportId, failure };
}
