use std::net::Ipv4Addr;

use tcpip_userland::Host;
use tcpip_userland::application::run_until_idle;
use tcpip_userland::link::SharedMedium;
use tcpip_userland::link::ethernet::MacAddr;

pub fn run() {
    let medium = SharedMedium::new();
    let mut hosts = [
        Host::new(
            "alpha",
            Ipv4Addr::new(10, 0, 0, 1),
            MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
            &medium,
        ),
        Host::new(
            "beta",
            Ipv4Addr::new(10, 0, 0, 2),
            MacAddr::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]),
            &medium,
        ),
    ];
    let conn1;
    let conn2;
    {
        let (left, right) = hosts.split_at_mut(1);
        let host_a = &mut left[0];
        let host_b = &mut right[0];

        host_b.open_udp(8080);
        host_b.open_udp(8081);
        host_b.listen_tcp(9000);

        host_a.send_udp(50000, host_b.ip(), 8080, b"udp-message-1".to_vec());
        host_a.send_udp(50001, host_b.ip(), 8081, b"udp-message-2".to_vec());
        let _ = host_a.ping(host_b.ip(), b"ping".to_vec());

        conn1 = host_a.connect_tcp_from(40000, host_b.ip(), 9000);
        conn2 = host_a.connect_tcp_from(40001, host_b.ip(), 9000);
    }
    run_until_idle(&mut hosts);

    let server_conn1;
    let server_conn2;
    {
        let (left, right) = hosts.split_at_mut(1);
        let host_a = &mut left[0];
        let host_b = &mut right[0];

        server_conn1 = host_b.accept_tcp(9000).expect("first connection");
        server_conn2 = host_b.accept_tcp(9000).expect("second connection");

        let _ = host_a.send_tcp(conn1, b"tcp-message-1".to_vec());
        let _ = host_a.send_tcp(conn2, b"tcp-message-2".to_vec());
    }
    run_until_idle(&mut hosts);

    {
        let (left, right) = hosts.split_at_mut(1);
        let host_a = &mut left[0];
        let host_b = &mut right[0];

        let udp1 = host_b.recv_udp(8080).expect("udp 8080");
        let udp2 = host_b.recv_udp(8081).expect("udp 8081");
        let ping = host_a.recv_ping_reply().expect("icmp echo reply");
        let tcp1 = host_b.recv_tcp(server_conn1).expect("tcp 1");
        let tcp2 = host_b.recv_tcp(server_conn2).expect("tcp 2");

        println!("udp/8080 <- {}", String::from_utf8_lossy(&udp1.payload));
        println!("udp/8081 <- {}", String::from_utf8_lossy(&udp2.payload));
        println!("icmp <- {}", String::from_utf8_lossy(&ping.payload));
        println!("tcp/1 <- {}", String::from_utf8_lossy(&tcp1));
        println!("tcp/2 <- {}", String::from_utf8_lossy(&tcp2));
    }
}
