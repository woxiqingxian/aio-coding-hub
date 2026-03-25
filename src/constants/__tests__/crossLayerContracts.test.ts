import { describe, expect, it } from "vitest";
import { appEventNames } from "../appEvents";
import { gatewayEventNames } from "../gatewayEvents";
import { GatewayErrorCodes } from "../gatewayErrorCodes";
import { HOME_USAGE_PERIOD_VALUES } from "../homeUsagePeriods";
import bindingsSource from "../../generated/bindings.ts?raw";
import heartbeatSource from "../../../src-tauri/src/app/heartbeat_watchdog.rs?raw";
import noticeSource from "../../../src-tauri/src/app/notice.rs?raw";
import gatewayEventsSource from "../../../src-tauri/src/gateway/events.rs?raw";
import gatewayErrorCodeSource from "../../../src-tauri/src/gateway/proxy/error_code.rs?raw";

function extractRustStringConst(source: string, constName: string) {
  const match = source.match(new RegExp(`const\\s+${constName}:\\s*&str\\s*=\\s*"([^"]+)"`));
  expect(match, `missing Rust const ${constName}`).toBeTruthy();
  return match?.[1] ?? "";
}

function extractBindingsUnionLiterals(source: string, typeName: string) {
  const match = source.match(new RegExp(`export type ${typeName} = ([^;]+);`));
  expect(match, `missing generated type ${typeName}`).toBeTruthy();
  return Array.from((match?.[1] ?? "").matchAll(/"([^"]+)"/g), (part) => part[1]);
}

function extractRustGatewayErrorCodes(source: string) {
  return Array.from(
    new Set(
      Array.from(source.matchAll(/"((?:GW|CLI_PROXY)_[A-Z0-9_]+)"/g), (match) => match[1]).filter(
        (value) => value !== "GW_UNKNOWN"
      )
    )
  );
}

describe("cross-layer contracts", () => {
  it("keeps app event names aligned with Rust emitters", () => {
    expect(extractRustStringConst(heartbeatSource, "HEARTBEAT_EVENT_NAME")).toBe(
      appEventNames.heartbeat
    );
    expect(extractRustStringConst(noticeSource, "NOTICE_EVENT_NAME")).toBe(appEventNames.notice);
  });

  it("keeps gateway event names aligned with Rust emitters", () => {
    expect(extractRustStringConst(gatewayEventsSource, "GATEWAY_STATUS_EVENT_NAME")).toBe(
      gatewayEventNames.status
    );
    expect(extractRustStringConst(gatewayEventsSource, "GATEWAY_REQUEST_START_EVENT_NAME")).toBe(
      gatewayEventNames.requestStart
    );
    expect(extractRustStringConst(gatewayEventsSource, "GATEWAY_ATTEMPT_EVENT_NAME")).toBe(
      gatewayEventNames.attempt
    );
    expect(extractRustStringConst(gatewayEventsSource, "GATEWAY_REQUEST_EVENT_NAME")).toBe(
      gatewayEventNames.request
    );
    expect(extractRustStringConst(gatewayEventsSource, "GATEWAY_LOG_EVENT_NAME")).toBe(
      gatewayEventNames.log
    );
    expect(extractRustStringConst(gatewayEventsSource, "GATEWAY_CIRCUIT_EVENT_NAME")).toBe(
      gatewayEventNames.circuit
    );
  });

  it("keeps gateway error codes aligned with Rust definitions", () => {
    expect(extractRustGatewayErrorCodes(gatewayErrorCodeSource)).toEqual(
      Object.values(GatewayErrorCodes)
    );
  });

  it("keeps generated HomeUsagePeriod literals aligned with shared frontend values", () => {
    expect(extractBindingsUnionLiterals(bindingsSource, "HomeUsagePeriod")).toEqual([
      ...HOME_USAGE_PERIOD_VALUES,
    ]);
  });
});
