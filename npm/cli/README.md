# @yuzuko/cli

The `yuzuko` command for the [Yuzuko](https://github.com/petamoriken/yuzuko) runtime.

```sh
npm install -g @yuzuko/cli
yuzuko --help
```

`@yuzuko/cli` requires Node.js 22 or newer. It lists `@yuzuko/cli-darwin-arm64`, `@yuzuko/cli-darwin-x64`, `@yuzuko/cli-linux-arm64`, `@yuzuko/cli-linux-x64`, and `@yuzuko/cli-win32-x64` as optional dependencies. Each of those packages contains the executable for one platform. npm installs the package that matches this machine, and the `yuzuko` command runs that file. A Linux install needs the GTK 3, libxdo, libudev, libayatana-appindicator, and ALSA shared libraries.
