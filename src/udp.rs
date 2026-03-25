use skyline::libc::{socket, setsockopt, sendto, sockaddr_in, in_addr, AF_INET, SOCK_DGRAM, SOL_SOCKET, SO_BROADCAST};
use core::mem;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicI32, Ordering};
use skyline::libc::close;

static BROADCAST_SOCKET: AtomicI32 = AtomicI32::new(-1);
const BROADCAST_PORT: u16 = 6500;

/// From nn::nifm - already defined in your original
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct InAddr {
    pub s_addr: u32,
}
extern "C" {
    #[link_name = "\u{1}_ZN2nn4nifm26GetCurrentPrimaryIpAddressEP7in_addr"]
    fn GetCurrentPrimaryIpAddress(addr: *mut InAddr) -> i32;
}

fn get_ip_addr() -> Option<Ipv4Addr> {
    let mut addr = InAddr { s_addr: 0 };

    let res = unsafe { GetCurrentPrimaryIpAddress(&mut addr) };

    if res == 0 {
        let ip = Ipv4Addr::from(addr.s_addr.to_be());
        Some(ip)
    } else {
        println!("[smush_info] Failed to get IP address, error code: {}", res);
        None
    }
}

fn get_username() -> Option<String> {
    unsafe {
        nnsdk::account::Initialize();
    }

    let users = nnsdk::account::list_all_users();
    if users.is_empty() {
        println!("[smush_info] No users found.");
        return None;
    }

    for uid in &users {
        let nickname = nnsdk::account::get_nickname(uid);
        if !nickname.is_empty() {
            return Some(nickname);
        }
    }

    println!("[smush_info] Failed to get any valid username.");
    None
}

fn get_broadcast_socket() -> Option<i32> {
    let sock_fd = BROADCAST_SOCKET.load(Ordering::Relaxed);
    if sock_fd >= 0 {
        return Some(sock_fd);
    }

    unsafe {
        let sock = socket(AF_INET, SOCK_DGRAM, 0);
        if sock < 0 {
            println!("[smush_info] Failed to create socket");
            return None;
        }

        // Enable broadcast
        let optval: i32 = 1;
        if setsockopt(sock, SOL_SOCKET, SO_BROADCAST, &optval as *const _ as *const _, mem::size_of::<i32>() as u32) < 0 {
            println!("[smush_info] setsockopt SO_BROADCAST failed");
        }

        BROADCAST_SOCKET.store(sock, Ordering::Relaxed);
        Some(sock)
    }
}

pub fn broadcast_device_info() {
    unsafe {
        let sock = match get_broadcast_socket() {
            Some(s) => s,
            None => return,
        };

        let ip = match get_ip_addr() {
            Some(ip) => ip,
            None => return,
        };

        let username = match get_username() {
            Some(name) => name,
            None => return,
        };

        let message = format!("smush_info:{}:{}", username, ip);
        let message_bytes = message.as_bytes();

        let ip_octets = ip.octets();
        let broadcast_ip = Ipv4Addr::new(ip_octets[0], ip_octets[1], ip_octets[2], 255);

        let addr = sockaddr_in {
            sin_len: mem::size_of::<sockaddr_in>() as u8,
            sin_family: AF_INET as u8,
            sin_port: BROADCAST_PORT.to_be(),
            sin_addr: in_addr { s_addr: u32::from_ne_bytes(broadcast_ip.octets()) },
            sin_zero: [0; 8],
        };

        let sent = sendto(
            sock,
            message_bytes.as_ptr() as *const _,
            message_bytes.len(),
            0,
            &addr as *const _ as *const _,
            mem::size_of::<sockaddr_in>() as u32,
        );

        if sent < 0 {
            let errno_ptr = skyline::libc::errno_loc();
            let errno_val = *(errno_ptr as *const i32);
            let err_str_ptr = skyline::libc::strerror(errno_val);
            let err_msg = std::ffi::CStr::from_ptr(err_str_ptr as *const u8)
                .to_str()
                .unwrap_or("unknown");

            println!(
                "[smush_info] Failed to send UDP packet: sendto returned {}, errno = {}, {}",
                sent, errno_val, err_msg
            );

            // Invalidate socket so next call recreates it
            let sock_fd = BROADCAST_SOCKET.swap(-1, Ordering::Relaxed);
            if sock_fd >= 0 {
                close(sock_fd);
                println!("[smush_info] Socket closed and will be recreated on next broadcast");
            }
        }

    }
}