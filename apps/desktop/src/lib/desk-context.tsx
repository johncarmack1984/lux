import { createContext, useContext } from "react";
import { setChannelValue, setFixtureCollapsed } from "@/lib/actions";
import { ownerPresetStore, type PresetStore } from "@/lib/preset-toggle";

/**
 * Where a control surface's writes land. The fixture components (cards, color
 * wheel, channel faders, preset row) are desk-agnostic: they read levels from
 * the buffer they're handed and send every write through this seam. The
 * default is the owner's local desk, so the fixtures and universe views need
 * no provider; the shared desk provides a guest implementation whose writes
 * cross the wire to the owner's rig.
 */
export type Desk = {
  /**
   * Whether the patch itself can be edited here — rename, readdress, delete.
   * A guest drives levels on someone else's fixtures; the patch is the
   * owner's, so a guest desk renders the same card read-only.
   */
  editable: boolean;
  /** Set one channel (1-based) to a value. */
  setChannel(channelNumber: number, value: number): Promise<unknown>;
  /** Persist one fixture card's collapse state. */
  setCollapsed(fixtureId: string, collapsed: boolean): Promise<unknown>;
  /** The preset lane state for this desk (see lib/preset-toggle). */
  presets: PresetStore;
};

const ownerDesk: Desk = {
  editable: true,
  setChannel: (channelNumber, value) => setChannelValue({ channelNumber, value }),
  setCollapsed: setFixtureCollapsed,
  presets: ownerPresetStore,
};

const DeskContext = createContext<Desk>(ownerDesk);

export const DeskProvider = DeskContext.Provider;

/** The desk this surface writes to; the owner's local desk by default. */
export const useDesk = (): Desk => useContext(DeskContext);
