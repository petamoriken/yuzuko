self.postMessage({
  clipboard: "clipboard" in navigator,
  Clipboard: "Clipboard" in self,
  DataTransfer: "DataTransfer" in self,
  ClipboardEvent: "ClipboardEvent" in self,
});
