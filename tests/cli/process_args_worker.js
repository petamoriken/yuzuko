self.onmessage = () => {
  postMessage(JSON.stringify(Yuzuko.args));
};
