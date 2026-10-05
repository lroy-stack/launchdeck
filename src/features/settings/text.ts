// The words of Settings that a test or more than one section uses: About's
// rows and the bug report made from them.

import type { AppInfo } from "../../lib/schema";

/** The license the owner chose (2026-10-05). */
export const LICENSE = "MIT";

const SYSTEMS: Record<string, string> = { macos: "macOS", windows: "Windows", linux: "Linux" };

/** The app's version and channel, as About's first line shows it. */
export const releaseOf = (info: AppInfo): string => `${info.appVersion} ${info.channel}`;

/** About's rows after the version, in order, as a label and a value. */
export function aboutRows(info: AppInfo): [string, string][] {
  const { oldest, newest } = info.manifestSchemas;
  return [
    ["System", `${SYSTEMS[info.os] ?? info.os}, ${info.arch}`],
    ["Plugin SDK", `${info.sdkVersion} (ABI ${info.abiVersion})`],
    ["Plugin manifests", oldest === newest ? `schema ${newest}` : `schemas ${oldest} to ${newest}`],
    ["Workspace", `schema ${info.workspaceSchema}`],
    ["Agent Bridge", `protocol ${info.bridgeProtocol}`],
    ["Surface and Pack files", `format ${info.artifactFormat}`],
    ["License", LICENSE],
  ];
}

/**
 * What "Copy for a bug report" puts on the clipboard: plain text, one fact per line.
 * The data folder is left out: its path usually holds the person's account name.
 */
export function bugReport(info: AppInfo): string {
  return [`LaunchDeck ${info.appVersion} (${info.channel})`, ...aboutRows(info).map(([label, value]) => `${label}: ${value}`)].join("\n");
}
