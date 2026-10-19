import { createContext, useContext } from "react";
import type { Json } from "./components";
export interface WorkspaceContext {
  tenant: string;
  tenantName: string;
  admin: string;
  revision: number;
  providers: Json[];
  connection: string;
  model: string;
  models: Json[];
  setConnection: (v: string) => void;
  setModel: (v: string) => void;
  refreshProviders: () => void;
  refresh: () => void;
  openKnowledge: (id: string, version?: number) => void;
  openTask: (id: string) => void;
  notify: (text: string) => void;
}
export const Workspace = createContext<WorkspaceContext | null>(null);
export function useWorkspace() {
  const value = useContext(Workspace);
  if (!value) throw new Error("Workspace context is missing");
  return value;
}
