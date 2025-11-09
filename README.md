# Ibuki

> Standalone Discord audio sending node written in Rust 

<p align="center">
    <img src="https://azurlane.netojuu.com/images/thumb/2/2d/IbukiCasual.png/587px-IbukiCasual.png"> 
</p>

> Artwork from Azur Lane

## Features

- Music
  - [x] Playback
  - [x] Seek
  - [x] Pause/Resume
  - [x] Volume
  - [ ] Filters 
    - Filters support may take time (a lot of time) due to Songbird not supporting it means I need to implement it from scratch
    - If you are willing to implement this however, feel free and open a PR once you are able
- Websocket
  - [x] Connect
  - [x] Disconnect
  - [x] Resumes
- Rest
  - [x] Get Player
  - [x] Update Player
  - [x] Delete Player
  - [x] Update Session
    - Used to configure resuming capabilities
  - [x] Decode
  - [x] Encode
- Client Support
  - Any client that has support for Lavalink v4 will work. Do note that only the endpoints I mentioned in Rest part of this readme are supported, means other endpoints will return 404
  - One example client that has support for Lavalink v4 is [Shoukaku](https://github.com/shipgirlproject/Shoukaku) and will work as a drop in replacement

- Cases you may want to try Ibuki
  - You want something that runs natively on your system without additional overhead

## Sources

- [x] Youtube
  - Support is available via `RustyPipe`, and `Ytdlp` support will be added in future
- [x] Deezer
  - Only search is supported `dzisrc:` `dzisearch:`. Link loading is wip
- [ ] Soundcloud
- [x] Http

> Support for more sources will come in future as I free more time to work on this project. Feel free to open a PR if you want to implement a new source
  
## Downloads

- Keep in mind that **Ibuki is in its alpha state**, and for any users that wants to try it is welcome to do so. For issues, please open an issue in [Issues Tab](https://github.com/Deivu/Ibuki/issues)
  - Windows Download: [x86_64-pc-windows-msvc](https://github.com/Deivu/Ibuki/actions/runs/19212990662/artifacts/4513085588)
  - Linux Download: [x86_64-unknown-linux-gnu](https://github.com/Deivu/Ibuki/actions/runs/19212990662/artifacts/4513073111)
    - These downloads are directly linked from [Github Actions](https://github.com/Deivu/Ibuki/actions) latest run. You can always build the project if you don't want to download the binaries
- Docker support will come in future

## Contributing
- The dev enviroment used in this project is
  - Windows
  - Rust toolchain: `nightly-x86_64-pc-windows-msvc`
  - Cmake: `3.31.7`
- This should enable you to fork, compile and test the project before opening a PR

## Configuration

- Just put a `config.json` beside the executable
- An example config file is available at [example-config.json](https://github.com/Deivu/Ibuki/blob/master/example-config.json) or below

```json
{
    "port": 8080,
    "address": "0.0.0.0",
    "authorization": "heavy-cruiser-ibuki",
    "playerUpdateSecs": 30,
    "statusUpdateSecs": 10,
    "deezerConfig": {
        "decryptKey": "your-decrypt-key",
        "arl": "your-arl-token"
    },
    "youtubeConfig": {
        "usePoToken": true,
        "useOauth": false,
        "cookies": "your-cookies-string"
    },
    "httpConfig": {}
}
```

- Source configuration like `deezerConfig`, `youtubeConfig`, and `httpConfig` can be disabled by removing them from the json. Here is an example below if we only want to enable `httpConfig`
  - Do note that httpConfig don't have additional configuration for now, hence if you want to enable it, leaving an empty object will do.

```json
{
    "port": 8080,
    "address": "0.0.0.0",
    "authorization": "heavy-cruiser-ibuki",
    "playerUpdateSecs": 30,
    "statusUpdateSecs": 10,
    "httpConfig": {}
}
```

If you need help or ask for help or something, feel free to join our [Discord Server](https://discord.gg/FVqbtGu) and just ping `@ichimakase (Saya)` in `#general` channel or open a thread in `#development-support` forum


### Made with ❤ by @ichimakase (Saya)

> The Shipgirl Project