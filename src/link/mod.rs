pub mod arp;
pub mod ethernet;
pub mod switch;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::rc::Rc;

use ethernet::{EthernetFrame, MacAddr};

#[derive(Clone)]
pub struct SharedMedium {
    inner: Rc<RefCell<MediumState>>,
}

#[derive(Default)]
struct MediumState {
    queues: HashMap<MacAddr, VecDeque<EthernetFrame>>,
}

impl SharedMedium {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(MediumState::default())),
        }
    }

    pub fn connect(&self, mac: MacAddr) -> LinkEndpoint {
        self.inner.borrow_mut().queues.entry(mac).or_default();
        LinkEndpoint {
            inner: LinkEndpointInner::Shared(SharedLinkEndpoint {
                mac,
                medium: self.clone(),
            }),
        }
    }

    fn transmit(&self, frame: EthernetFrame) {
        let mut inner = self.inner.borrow_mut();
        if frame.dst == MacAddr::BROADCAST {
            for (mac, queue) in &mut inner.queues {
                if *mac != frame.src {
                    queue.push_back(frame.clone());
                }
            }
            return;
        }

        if let Some(queue) = inner.queues.get_mut(&frame.dst) {
            queue.push_back(frame);
        }
    }

    fn recv(&self, mac: MacAddr) -> Option<EthernetFrame> {
        self.inner.borrow_mut().queues.get_mut(&mac)?.pop_front()
    }
}

#[derive(Clone)]
pub struct LinkEndpoint {
    inner: LinkEndpointInner,
}

#[derive(Clone)]
enum LinkEndpointInner {
    Shared(SharedLinkEndpoint),
    Udp(Rc<UdpLinkEndpoint>),
}

#[derive(Clone)]
struct SharedLinkEndpoint {
    mac: MacAddr,
    medium: SharedMedium,
}

struct UdpLinkEndpoint {
    socket: UdpSocket,
    uplink: RefCell<Option<SocketAddr>>,
    local_mac: MacAddr,
}

impl LinkEndpoint {
    pub fn udp(bind_addr: SocketAddr, uplink_addr: SocketAddr, mac: MacAddr) -> io::Result<Self> {
        let socket = UdpSocket::bind(bind_addr)?;
        socket.set_nonblocking(true)?;
        let endpoint = Rc::new(UdpLinkEndpoint {
            socket,
            uplink: RefCell::new(Some(uplink_addr)),
            local_mac: mac,
        });
        endpoint.announce();
        Ok(Self {
            inner: LinkEndpointInner::Udp(endpoint),
        })
    }

    pub fn udp_port(bind_addr: SocketAddr, mac: MacAddr) -> io::Result<Self> {
        let socket = UdpSocket::bind(bind_addr)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            inner: LinkEndpointInner::Udp(Rc::new(UdpLinkEndpoint {
                socket,
                uplink: RefCell::new(None),
                local_mac: mac,
            })),
        })
    }

    pub fn send(&self, frame: EthernetFrame) {
        match &self.inner {
            LinkEndpointInner::Shared(endpoint) => endpoint.medium.transmit(frame),
            LinkEndpointInner::Udp(endpoint) => {
                endpoint.announce();
                let mut packet = Vec::with_capacity(1 + 14 + frame.payload.len());
                packet.push(0);
                packet.extend_from_slice(&frame.encode());
                if let Some(uplink) = endpoint.uplink() {
                    let _ = endpoint.socket.send_to(&packet, uplink);
                }
            }
        }
    }

    pub fn recv(&self) -> Option<EthernetFrame> {
        match &self.inner {
            LinkEndpointInner::Shared(endpoint) => endpoint.medium.recv(endpoint.mac),
            LinkEndpointInner::Udp(endpoint) => {
                endpoint.announce();
                let mut buffer = [0u8; 2048];
                match endpoint.socket.recv_from(&mut buffer) {
                    Ok((len, remote)) if len >= 7 && buffer[0] == 1 => {
                        endpoint.learn_peer(remote);
                        None
                    }
                    Ok((len, remote)) if len > 1 && buffer[0] == 0 => {
                        endpoint.learn_peer(remote);
                        EthernetFrame::decode(&buffer[1..len])
                    }
                    Ok(_) => None,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => None,
                    Err(_) => None,
                }
            }
        }
    }

    pub fn local_addr(&self) -> io::Result<Option<SocketAddr>> {
        match &self.inner {
            LinkEndpointInner::Shared(_) => Ok(None),
            LinkEndpointInner::Udp(endpoint) => endpoint.socket.local_addr().map(Some),
        }
    }
}

impl UdpLinkEndpoint {
    fn uplink(&self) -> Option<SocketAddr> {
        *self.uplink.borrow()
    }

    fn learn_peer(&self, peer: SocketAddr) {
        *self.uplink.borrow_mut() = Some(peer);
    }

    fn announce(&self) {
        let Some(uplink) = self.uplink() else {
            return;
        };
        let mut register_packet = Vec::with_capacity(7);
        register_packet.push(1);
        register_packet.extend_from_slice(&self.local_mac.octets());
        let _ = self.socket.send_to(&register_packet, uplink);
    }
}
