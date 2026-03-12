# tcpip_userland

Linuxユーザ空間で動くTCP/IPスタックをRustで自作するための最小実装です。RFC準拠よりも、レイヤ分離と複数通信を成立させることを優先しています。

## レイヤ構成

- `src/link`: Ethernet と ARP、共有メモリ上の仮想L2媒体
- `src/internet`: IPv4 と ICMP
- `src/transport`: UDP と簡易TCP
- `src/application`: ホスト、ソケット/接続テーブル、デモ実行補助

## 実装方針

- Linuxユーザ空間で扱いやすいように、まずは `SharedMedium` 上でフレームをやり取りする
- 別プロセス間では localhost UDP ソケット上にEthernetフレームをそのまま流せる
- ARP未解決時はIPパケットを保留し、Reply後にフラッシュする
- UDPはポート単位の受信キューで多重化する
- TCPは4タプル単位の接続テーブルで多重化する
- TCPは `SYN -> SYN/ACK -> ACK` とデータACKのみを扱う簡易実装

## L2スイッチ経由の通信

Gateway terminal:

```bash
cargo run -- gateway
```

- `gateway` は内部で L2スイッチとDHCPサーバをまとめて起動する
- `show mac`, `show ports`, `show leases` が使える
- 必要なら `switch` と `dhcp-server` を個別に起動して検証もできる

## 2端末での動かし方

Terminal 1:

```bash
cargo run -- chat 02:00:00:00:00:01 7000
```

Terminal 2:

```bash
cargo run -- chat 02:00:00:00:00:02 7000
```

- `7000` はそのホストが待ち受けるUDPポート
- `cargo run -- chat 02:00:00:00:01:0a 7000 02:00:00:01:01:01` のように、短い書式の 3 引数目へ uplink 側の router MAC を足せる
- `cargo run -- chat 02:00:00:00:00:01 7000 10.0.0.254` のように、短い書式の末尾へ default gateway を足す後方互換も残している
- `src-port` は省略でき、省略時は `listen-port` と同じ値を使う
- `send 10.0.0.2 7000 hello` のように宛先IP/ポートをその場で指定して送る
- `/ping 10.0.0.2` のように宛先IPを指定してICMP Echoを投げる
- `/quit` で終了する
- ホスト同士は直接つながず、L2スイッチにだけ接続する
- スイッチ側では `show mac` と `show ports` が使える

## HTTP通信

Server terminal:

```bash
cargo run -- http-server 02:00:00:00:00:01 8080 02:00:00:01:01:01
```

Client terminal:

```bash
cargo run -- http-get 10.0.0.1 02:00:00:00:00:02 8080 /hello 02:00:00:01:01:01
```

- `http-server` は簡易HTTPサーバとして `GET /` と `GET /hello` を返す
- `http-get` は簡易HTTPクライアントとしてレスポンス全文を表示する
- どちらも内部ではこの実装のTCPを使う
- `wan.toml` があると `http-server <mac> <port> <uplink-mac>` と `http-get <peer-ip> <mac> <port> <path> <uplink-mac>` で uplink と DHCP を自動解決できる
- UDPのbindポートとスイッチの待受ポートは `127.0.0.1:0` で自動採番される
- スイッチの実アドレスは `.tcpip_switch_addr` に保存され、各ホストはそこから自動で参照する

## DHCP

DHCP client:

```bash
cargo run -- dhcp-client 02:00:00:00:00:01
```

- `DISCOVER -> OFFER -> REQUEST -> ACK` の簡易DHCPをUDPで実装している
- `255.255.255.255` 宛てIPパケットはL2ブロードキャストとして流れる
- `gateway` は `MAC -> IP` のリーステーブルを持つ
- `dhcp-client` は単体確認用で、普段は `chat` / `http-server` / `http-get` が起動時に自分でDHCPを取りに行く
- そのため、通常の利用ではリース用の一時ファイルは不要
- デフォルトの lease は 60 秒で、`chat` / `http-server` / `http-get` はおよそ 45 秒時点で自動更新する
- 更新に失敗して lease が切れた場合も、gateway が戻れば自動で再取得する
- `chat` と `http-get` は終了時に DHCP RELEASE を送り、`http-server` も `/quit` で終了すると RELEASE を送る

## L3ルーター経由の通信

3つのL2スイッチを挟んで、2台のルーターで別セグメントをつなげられます。

Switch 1:

```bash
cargo run -- switch 127.0.0.1:39001
```

Switch 2:

```bash
cargo run -- switch 127.0.0.1:39002
```

Switch 3:

```bash
cargo run -- switch 127.0.0.1:39003
```

Router 1:

```bash
cargo run -- router 02:00:00:ff:00:01
```

Router 2:

```bash
cargo run -- router 02:00:00:ff:00:02
```

Chat 1:

```bash
cargo run -- chat 02:00:00:00:01:0a 7000 02:00:00:01:01:01
```

Chat 2:

```bash
cargo run -- chat 02:00:00:00:02:0a 7000 02:00:00:02:02:01
```

- `router` は2インターフェース固定の簡易L3ルーターで、connected route は自動で入る
- `wan.toml` があれば `cargo run -- router <router-mac>` だけで起動できる
- 各インターフェースは `127.0.0.1:39001` のような uplink 指定か、`listen` のどちらかを取れる
- `listen` な interface は DHCP を内蔵し、その interface 自身の IP を default gateway として配る
- 追加の静的ルートは `<cidr> <next-hop|direct>` を2つ組で末尾に足せる
- `next-hop` がどのサブネットにいるかを見て、出るインターフェースは自動で決まる
- `show routes`, `show ifaces`, `/quit` が使える
- `chat <host-mac> <port> <uplink-mac>` なら uplink と DHCP を自動解決して、別セグメント宛ての `send` と `/ping` をそのまま通せる

### switch なしで `router x2 + chat x2`

Router 1:

```bash
cargo run -- router 127.0.0.1:40011 listen 10.0.1.1 02:00:00:01:01:01 127.0.0.1:40012 127.0.0.1:40021 10.0.12.1 02:00:00:01:0c:01 10.0.2.0/24 10.0.12.2
```

Router 2:

```bash
cargo run -- router 127.0.0.1:40021 127.0.0.1:40012 10.0.12.2 02:00:00:02:0c:02 127.0.0.1:40022 listen 10.0.2.1 02:00:00:02:02:01 10.0.1.0/24 10.0.12.1
```

Chat 1:

```bash
cargo run -- chat 02:00:00:00:01:0a 7000 02:00:00:01:01:01
```

Chat 2:

```bash
cargo run -- chat 02:00:00:00:02:0a 7000 02:00:00:02:02:01
```

- `listen` 側の router interface は最初に接続してきた host の UDP アドレスを学習する
- `wan.toml` があると `chat <mac> <listen-port> <uplink-mac>` から uplink を自動解決できる
- 旧来の `chat <mac> <listen-port> <router-ip>` も使える
- これで `chat1 -> router1 -> router2 -> chat2` の point-to-point 構成を switch なしで作れる

### wan.toml

ルーターの bind/uplink/IP/MAC と static route は [`wan.toml`](/Users/chikina/workspace/production/tcpip/wan.toml) に書いてある。

### WAN を挟むイメージ

WAN terminal:

```bash
cargo run -- wan 127.0.0.1:39010
```

- `wan` は中身は L2 媒体だが、役割としてはルーター同士をぶら下げる共通セグメントとして使える
- その場合も route は `next-hop` だけ書けばよく、WAN 側の ARP 解決は自動で流れる

## 次に伸ばすポイント

- `SharedMedium` を TUN/TAP バックエンドに差し替える
- TCP再送、FIN/RST、輻輳制御、順序入れ替わり対応
- IPフラグメントや複数ルート/ACL
- ソケットAPI風の外部インタフェース
