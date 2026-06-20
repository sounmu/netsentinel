"use client";

import { Toaster } from "sonner";

export default function ClientToaster() {
  return <Toaster position="top-right" theme="system" richColors duration={4000} />;
}
