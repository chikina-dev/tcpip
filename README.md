# tcpip_userland

Linuxユーザ空間で動くTCP/IPスタックをRustで自作するための最小実装です。RFC準拠よりも、レイヤ分離と複数通信を成立させることを優先しています。

## レイヤ構成

- `src/link`: Ethernet と ARP、共有メモリ上の仮想L2媒体
- `src/internet`: IPv4 と ICMP
- `src/transport`: UDP と簡易TCP
- `src/application`: ホスト、ソケット/接続テーブル、デモ実行補助
