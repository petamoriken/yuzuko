const onSignal = () => {
  Yuzuko.removeSignalListener("SIGINT", onSignal);
  postMessage({ hit: true });
};
Yuzuko.addSignalListener("SIGINT", onSignal);
postMessage({ ready: true });
