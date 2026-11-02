self.onmessage = async (event) => {
  try {
    const child = Yuzuko.spawn(event.data.command, { args: event.data.args });
    const output = await child.output();
    postMessage({
      code: output.code,
      stdout: new TextDecoder().decode(output.stdout),
    });
  } catch (error) {
    postMessage({
      error: error instanceof Error ? error.message : String(error),
    });
  }
};
postMessage({ ready: true });
