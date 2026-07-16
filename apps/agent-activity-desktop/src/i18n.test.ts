import { describe, expect, it } from "vitest";
import { translate } from "./i18n";
import { floatingStatusLabelKey } from "./status";
import type { SessionStatus } from "./types";

describe("floating traffic light status translations", () => {
  it("provides compact English and Chinese labels for every session status", () => {
    const statuses: SessionStatus[] = [
      "offline",
      "idle",
      "working",
      "waiting_approval",
      "complete",
      "error",
      "sleeping",
    ];

    expect(statuses.map((status) => translate("en", floatingStatusLabelKey[status]))).toEqual([
      "Offline",
      "Idle",
      "Working",
      "Approval",
      "Done",
      "Error",
      "Sleeping",
    ]);
    expect(statuses.map((status) => translate("zh", floatingStatusLabelKey[status]))).toEqual([
      "离线",
      "空闲",
      "工作中",
      "待审批",
      "已完成",
      "异常",
      "休眠",
    ]);
  });
});
