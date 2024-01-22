#![feature(proc_macro_hygiene)]
#![feature(simd_ffi)]

use skyline::hooks::{getRegionAddress, Region, InlineCtx};
use skyline::from_c_str;
use skyline::libc::*;
use std::time::Duration;
use std::mem::size_of_val;
use std::sync::atomic::Ordering;

use smash::app;
use smash::app::lua_bind;
use smash::app::lua_bind::*;
use smash::lib::lua_const::*;
use smash::app::BattleObject;
use smash::lua2cpp::{L2CFighterCommon, L2CFighterCommon_status_pre_Rebirth, L2CFighterCommon_status_pre_Entry, L2CFighterCommon_sub_damage_uniq_process_init, L2CFighterCommon_status_pre_Dead};
use smash::lib::L2CValue;

use smush_info_shared::Info;

use core::arch::aarch64::*;
use smash::Vector3f;
use smash::Vector2f;

mod conversions;
use conversions::{kind_to_char, stage_id_to_stage};

static mut OFFSET1 : usize = 0x1b52a0;
static mut OFFSET2 : usize = 0x225dc2c;
static mut OFFSET3 : usize = 0xd7140;

// Default 13.0.1 offset
static mut FIGHTER_SELECTED_OFFSET: usize = 0x66e120;

static FIGHTER_SELECTED_SEARCH_CODE: &[u8] = &[
    0x04, 0xdc, 0x45, 0x94,
    0xe0, 0x03, 0x1c, 0x32,
    0xe1, 0x03, 0x1a, 0x32,
];

extern "C" {
    #[link_name = "\u{1}_ZN3app7utility8get_kindEPKNS_26BattleObjectModuleAccessorE"]
    pub fn get_kind(module_accessor: &mut app::BattleObjectModuleAccessor) -> i32;

    #[link_name = "\u{1}_ZN3app14sv_information8stage_idEv"]
    pub fn stage_id() -> i32;

    #[link_name = "\u{1}_ZN3app14sv_information27get_remaining_time_as_frameEv"]
    pub fn get_remaining_time_as_frame() -> u32;
    
    #[link_name = "\u{1}_ZN3app17sv_camera_manager15world_to_screenERKN3phx8Vector3fEb"]
    pub fn world_to_screen(vec: *const Vector3f, unk: bool) -> float32x2_t;
}

fn send_bytes(socket: i32, bytes: &[u8]) -> Result<(), i64> {
    unsafe {
        let ret = send(socket, bytes.as_ptr() as *const _, bytes.len(), 0);
        if ret < 0 {
            Err(*errno_loc())
        } else {
            Ok(())
        }
    }
}

fn as_pixels(vec: Vector3f) -> Vector2f {
    unsafe {        
        let screen = world_to_screen(&vec, true);
        let x = vget_lane_f32(screen, 0);
        let y = vget_lane_f32(screen, 1);
        return Vector2f{x, y};
    }
}

pub fn once_per_frame_per_fighter(fighter : &mut L2CFighterCommon) {
    let lua_state = fighter.lua_state_agent;
    let module_accessor = unsafe { app::sv_system::battle_object_module_accessor(lua_state) };
    
    unsafe {
        let entry_id = WorkModule::get_int(module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID) as i32;
        let player_num = entry_id as usize;
        let pos_x = lua_bind::PostureModule::pos_x(module_accessor);
        let pos_y = lua_bind::PostureModule::pos_y(module_accessor);
        let pos_z = lua_bind::PostureModule::pos_z(module_accessor);
        println!("player {} x {} y {} z {}", player_num, pos_x, pos_y, pos_z);
        let pos = Vector3f { x: pos_x, y: pos_y, z: pos_z };
        let screen_pos = as_pixels(pos);
        println!("player {} screen_pos x {} screen_pos y {}", player_num, screen_pos.x, screen_pos.y);
    
        GAME_INFO.players[player_num].x.store(screen_pos.x, Ordering::SeqCst);
        GAME_INFO.players[player_num].y.store(screen_pos.y, Ordering::SeqCst);
    }

}


static GAME_INFO: Info = Info::new();

#[allow(unreachable_code)]
fn start_server() -> Result<(), i64> {
    unsafe {
        let server_addr: sockaddr_in = sockaddr_in {
            sin_family: AF_INET as _,
            sin_port: 4242u16.to_be(),
            sin_len: 4,
            sin_addr: in_addr {
                s_addr: INADDR_ANY as _,
            },
            sin_zero: [0; 8],
        };

        let tcp_socket = socket(AF_INET, SOCK_STREAM, 0);

        macro_rules! dbg_err {
            ($expr:expr) => {
                let rval = $expr;
                if rval < 0 {
                    let errno = *errno_loc();
                    dbg!(errno);
                    close(tcp_socket);
                    return Err(errno);
                }
            };
        }

        if (tcp_socket as u32 & 0x80000000) != 0 {
            let errno = *errno_loc();
            dbg!(errno);
            return Err(errno);
        }

        let flags: u32 = 1;

        dbg_err!(setsockopt(
            tcp_socket,
            SOL_SOCKET,
            SO_KEEPALIVE,
            &flags as *const _ as *const c_void,
            size_of_val(&flags) as u32,
        ));

        dbg_err!(bind(
            tcp_socket,
            &server_addr as *const sockaddr_in as *const sockaddr,
            size_of_val(&server_addr) as u32,
        ));

        dbg_err!(listen(tcp_socket, 1));

        let mut addr_len: u32 = 0;

        let mut w_tcp_socket = accept(
            tcp_socket,
            &server_addr as *const sockaddr_in as *mut sockaddr,
            &mut addr_len,
        );

        loop {
            let mgr = *(FIGHTER_MANAGER_ADDR as *mut *mut app::FighterManager);
            let is_match = FighterManager::entry_count(mgr) > 0 &&
                !FighterManager::is_result_mode(mgr) &&
                *(offset_to_addr(0x53030f0) as *const u32) != 0x6020000; //is_match is set to true when the player in the controls screen, i assume because there is a sandbag and mario. this ensures we're not in the controls screen 

            if is_match {
                GAME_INFO.remaining_frames.store(get_remaining_time_as_frame(), Ordering::SeqCst);
                GAME_INFO.is_match.store(true, Ordering::SeqCst);
            } else {
                GAME_INFO.remaining_frames.store(-1.0 as u32, Ordering::SeqCst);
                GAME_INFO.is_match.store(false, Ordering::SeqCst);
            }

            GAME_INFO.current_menu.store(*(offset_to_addr(0x53030f0) as *const u32), Ordering::SeqCst);
            if(FighterManager::entry_count(mgr) > 0 && *(offset_to_addr(0x53030f0) as *const u32) != 0x6020000) {
                GAME_INFO.is_results_screen.store(FighterManager::is_result_mode(mgr), Ordering::SeqCst);
            }

            let mut data = serde_json::to_vec(&GAME_INFO).unwrap();
            data.push(b'\n');
            match send_bytes(w_tcp_socket, &data) {
                Ok(_) => (),
                Err(32) => {
                    w_tcp_socket = accept(
                        tcp_socket,
                        &server_addr as *const sockaddr_in as *mut sockaddr,
                        &mut addr_len,
                    );
                }
                Err(e) => {
                    println!("send_bytes errno = {}", e);
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        /*let magic = recv_bytes(tcp_socket, 4).unwrap();
        if &magic == b"HRLD" {
            let num_bytes = recv_u32(tcp_socket).unwrap();
        } else if &magic == b"ECHO" {
            println!("\n\n----\nECHO\n\n");
        } else {
            println!("Invalid magic")
        }*/
        
        dbg_err!(close(tcp_socket));
    }

    Ok(())
}

pub fn offset_to_addr(offset: usize) -> *const () {
    unsafe {
        (getRegionAddress(Region::Text) as *const u8).offset(offset as isize) as _
    }
}

#[inline(always)]
fn get_fp() -> *const u64 {
    let r;
    unsafe { std::arch::asm!("mov {0}, x29",out(reg) r) }
    r
}

#[skyline::hook(offset = OFFSET1)] //1
fn some_strlen_thing(x: usize) -> usize {
    unsafe {
        let y = (x + 0x18) as *const *const c_char;
        if !y.is_null() {
            let text = getRegionAddress(Region::Text) as u64;
            let lr_offset = *get_fp().offset(1) - text;
            if lr_offset == OFFSET2 as u64 { //2
                let arena_id = from_c_str(*y);
                if arena_id.len() == 5 {
                    GAME_INFO.arena_id.store_str(Some(&arena_id), Ordering::SeqCst);
                }
            }
        }
    }
    original!()(x)
}

static OFFSET1_SEARCH_CODE: &[u8] = &[ //add 38
    0x81, 0x0e, 0x40, 0xf9, //.text:00000071001B5268                 LDR             X1, [X20,#0x18] ; src
    0xe0, 0x03, 0x16, 0xaa  //.text:00000071001B526C                 MOV             X0, X22 ; dest
                            //.text:00000071001B5270                 MOV             X2, X21 ; n
                            //.text:00000071001B5274                 BL              memcpy_0
                            //.text:00000071001B5278                 LDR             X8, [X19,#0x18]
                            //.text:00000071001B527C                 STRB            WZR, [X8,X21]
                            //.text:00000071001B5280                 LDP             X29, X30, [SP,#0x30+var_s0]
                            //.text:00000071001B5284                 LDP             X20, X19, [SP,#0x30+var_10]
                            //.text:00000071001B5288                 LDP             X22, X21, [SP,#0x30+var_20]
                            //.text:00000071001B528C                 LDP             X24, X23, [SP+0x30+var_30],#0x40
                            //.text:00000071001B5290                 RET
                            //below is req function
                            //.text:00000071001B52A0                 LDR             X0, [X0,#0x18] ; s
                            //.text:00000071001B52A4                 B               strlen_0
];

static OFFSET2_SEARCH_CODE: &[u8] = &[ //add -C, just below is the address needed
                            //.text:000000710225DC2C                 MOV             X0, X20 ; this
                            //.text:000000710225DC30                 BL              _ZNSt3__115recursive_mutex6unlockEv_0 ; std::__1::recursive_mutex::unlock(void)
                            //.text:000000710225DC34                 LDR             X0, [X21]
    0x60, 0x02, 0x00, 0xb4, //.text:000000710225DC38                 CBZ             X0, loc_710225DC84
    0x14, 0x58, 0x40, 0xa9  //.text:000000710225DC3C                 LDP             X20, X22, [X0]
                            //.text:000000710225DC40                 LDR             X8, [X0,#0x10]!
                            //.text:000000710225DC44                 LDR             X8, [X8]
                            //.text:000000710225DC48                 BLR             X8
                            //.text:000000710225DC4C                 LDR             X8, [X27,#0x30]
                            //.text:000000710225DC50                 LDR             X8, [X8,#8]
                            //.text:000000710225DC54                 STR             X8, [X27,#0x20]
                            //.text:000000710225DC58                 CBNZ            X8, loc_710225DC64
                            //.text:000000710225DC5C                 LDR             X8, [X27,#0x28]
                            //.text:000000710225DC60                 STR             X8, [X27,#0x20]
];

#[skyline::hook(offset = OFFSET3)] //3, remained same somehow
fn close_arena(param_1: usize) {
    GAME_INFO.arena_id.store_str(None, Ordering::SeqCst);
    original!()(param_1);
}

static OFFSET3_SEARCH_CODE: &[u8] = &[ //exact
    0xff, 0x83, 0x01, 0xd1, //.text:00000071000D7140                 SUB             SP, SP, #0x60
    0xf6, 0x57, 0x03, 0xa9, //.text:00000071000D7144                 STP             X22, X21, [SP,#0x50+var_20]
    0xf4, 0x4f, 0x04, 0xa9, //.text:00000071000D7148                 STP             X20, X19, [SP,#0x50+var_10]
    0xfd, 0x7b, 0x05, 0xa9, //.text:00000071000D714C                 STP             X29, X30, [SP,#0x50+var_s0]
    0xfd, 0x43, 0x01, 0x91, //.text:00000071000D7150                 ADD             X29, SP, #0x50
    0x15, 0x44, 0x40, 0xf9  //.text:00000071000D7154                 LDR             X21, [X0,#0x88]
                            //.text:00000071000D7158                 MOV             X19, X0
                            //.text:00000071000D715C                 CBZ             X21, loc_71000D71B4
                            //.text:00000071000D7160                 LDR             X8, [X21,#0x10]
                            //.text:00000071000D7164                 CBZ             X8, loc_71000D71B4
                            //.text:00000071000D7168                 LDP             X8, X20, [X21]
                            //.text:00000071000D716C                 LDR             X9, [X8,#8]
                            //.text:00000071000D7170                 LDR             X10, [X20]
                            //.text:00000071000D7174                 STR             X9, [X10,#8]
                            //.text:00000071000D7178                 LDR             X8, [X8,#8]
                            //.text:00000071000D717C                 CMP             X20, X21
                            //.text:00000071000D7180                 STR             X10, [X8]
                            //.text:00000071000D7184                 STR             XZR, [X21,#0x10]
                            //.text:00000071000D7188                 B.EQ            loc_71000D71B4
];

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

pub static mut FIGHTER_MANAGER_ADDR: usize = 0;

pub unsafe fn set_player_information(module_accessor: &mut app::BattleObjectModuleAccessor) {
    let entry_id = WorkModule::get_int(module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID) as i32;
    let player_num = entry_id as usize;
    let mgr = *(FIGHTER_MANAGER_ADDR as *mut *mut app::FighterManager);
    let fighter_information = FighterManager::get_fighter_information(
        mgr, 
        app::FighterEntryID(entry_id)
    ) as *mut app::FighterInformation;

    let character = kind_to_char(get_kind(module_accessor)) as u32;
    let damage = DamageModule::damage(module_accessor, 0);
    let stock_count = FighterInformation::stock_count(fighter_information) as u32;
    let sd_count = FighterInformation::suicide_count(fighter_information, 0) as u32;
    let is_cpu = FighterInformation::is_operation_cpu(fighter_information);
    let skin = (WorkModule::get_int(module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_COLOR)) as u32; //returns costume slot 0-indexed

    if(FighterManager::entry_count(mgr) > 0) {
        GAME_INFO.players[player_num].hero_menu_selected.store(false, Ordering::SeqCst);
        GAME_INFO.players[player_num].hero_menu_open.store(false, Ordering::SeqCst);            
    }

    GAME_INFO.players[player_num].character.store(character, Ordering::SeqCst);
    GAME_INFO.players[player_num].damage.store(damage, Ordering::SeqCst);
    GAME_INFO.players[player_num].stocks.store(stock_count, Ordering::SeqCst);
    GAME_INFO.players[player_num].self_destructs.store(sd_count, Ordering::SeqCst);
    GAME_INFO.players[player_num].is_cpu.store(is_cpu, Ordering::SeqCst);
    GAME_INFO.players[player_num].skin.store(skin, Ordering::SeqCst);
    println!("ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ player tag {}", get_tag_of_player(player_num));
    GAME_INFO.players[player_num].name.store_str(Some(&get_tag_of_player(player_num)), Ordering::SeqCst);
}

#[skyline::hook(replace = L2CFighterCommon_status_pre_Entry)]
pub unsafe fn handle_pre_entry(fighter: &mut L2CFighterCommon) -> L2CValue {
    let module_accessor = app::sv_system::battle_object_module_accessor(fighter.lua_state_agent);
    set_player_information(module_accessor);

    GAME_INFO.stage.store(stage_id_to_stage(stage_id()) as u32, Ordering::SeqCst);

    original!()(fighter)
}

#[skyline::hook(replace = L2CFighterCommon_status_pre_Rebirth)]
pub unsafe fn handle_pre_rebirth(fighter: &mut L2CFighterCommon) -> L2CValue {
    let module_accessor = app::sv_system::battle_object_module_accessor(fighter.lua_state_agent);
    set_player_information(module_accessor);

    original!()(fighter)
}

#[skyline::hook(replace = L2CFighterCommon_status_pre_Dead)]
pub unsafe fn handle_pre_dead(fighter: &mut L2CFighterCommon) -> L2CValue { // this kinda fucking sucks but whatever
    let module_accessor = app::sv_system::battle_object_module_accessor(fighter.lua_state_agent);

    let entry_id = WorkModule::get_int(module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID) as i32;
    let player_num = entry_id as usize;
    let mgr = *(FIGHTER_MANAGER_ADDR as *mut *mut app::FighterManager);
    let fighter_information = FighterManager::get_fighter_information(
        mgr, 
        app::FighterEntryID(entry_id)
    ) as *mut app::FighterInformation;

    let stock_count = (FighterInformation::stock_count(fighter_information) as u32) - 1;
    GAME_INFO.players[player_num].stocks.store(stock_count, Ordering::SeqCst);

    println!("ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ WE CALLED L2CFighterCommon_status_pre_Dead AND ATTEMPTED TO SET STOCK COUNT TO {}", stock_count);
    // set_player_information(module_accessor);

    original!()(fighter)
}

#[skyline::hook(replace = L2CFighterCommon_sub_damage_uniq_process_init)]
pub unsafe fn handle_sub_damage_uniq_process_init(fighter: &mut L2CFighterCommon) -> L2CValue {
    let module_accessor = app::sv_system::battle_object_module_accessor(fighter.lua_state_agent);
    set_player_information(module_accessor);

    original!()(fighter)
}

fn nro_main(nro: &skyline::nro::NroInfo<'_>) {
    match nro.name {
        "common" => {
            skyline::install_hooks!(
                handle_pre_entry,
                handle_pre_rebirth,
                handle_sub_damage_uniq_process_init,
                handle_pre_dead
            );
        }
        _ => (),
    }
}


static UPDATE_TAG_FOR_PLAYER_OFFSET: usize = 0x19fc5b0;
static PLAYER_SAVE_OFFSET: usize = 0x5312510;
static mut PLAYER_SAVE_ADDRESS: *const u64 = 0x0 as *const u64;

static PLAYER_TAG_OFFSET: usize = 0x52c3758;

pub fn get_tag_of_player(player_index: usize) -> String {
    let player_tag_offset = PLAYER_TAG_OFFSET + (player_index * 0x260);
    let player_tag_addr: *const u16 = offset_to_addr(player_tag_offset) as *const u16;
    
    unsafe {
        let mut len = 0;
        while *player_tag_addr.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(player_tag_addr, len);
        String::from_utf16_lossy(slice)
    }
}

pub fn get_tag_from_save(tag_index: u8) -> String {
    unsafe {
        let addr = (***((*((*PLAYER_SAVE_ADDRESS) as *const u64) + 0x58) as *const *const *const u64) + ((tag_index as u64) * 0xF7D8) + 0xC) as *const u16;
        let mut len = 0;
        while *addr.add(len) != 0 {
            len += 1;
        }

        let slice = std::slice::from_raw_parts(addr, len);
        String::from_utf16_lossy(slice)
    }
}

#[skyline::hook(offset = UPDATE_TAG_FOR_PLAYER_OFFSET)]
pub fn update_tag_for_player(param_1: u64, tag_index: *const u8){
    unsafe {
        let player_index = *((param_1 as *mut u8).offset(0x1d4) as *mut i32) as usize;
        GAME_INFO.players[player_index].name.store_str(Some(&get_tag_from_save(*tag_index)), Ordering::SeqCst);
        
        println!("AAAAAAAAAAAAAAAAAAAA PLAYER NAME OF INDEX {} IS {}", player_index, get_tag_from_save(*tag_index));
        call_original!(param_1, tag_index);
    }
}

#[derive(Debug)]
struct UnkPtr1 {
    ptrs: [&'static u64; 7],
}

#[derive(Debug)]
struct UnkPtr2 {
    bunch_bytes: [u8; 0x20],
    bunch_bytes2: [u8; 0x20]
}

#[derive(Debug)]
#[repr(C)]
pub struct FighterInfo {
    unk_ptr1: &'static UnkPtr1,
    unk_ptr2: &'static UnkPtr2,
    unk1: [u8; 0x20],
    unk2: [u8; 0x20],
    unk3: [u8; 0x8],
    fighter_id: u8,
    unk4: [u8;0xB],
    fighter_slot: u8,
}

#[derive(Debug)]
#[repr(C)]
pub struct FighterInfoBasic {
    field0_0x0: *mut (),
    field1_0x8: *mut (),
    field2_0x10: [u8; 0x30],
    ice_climber_going_first: u32,
    field67_0x54: u32,
    fighter_id: u32,
    redirected_fighter_id: u32,
    field70_0x60: u32,
    fighter_slot: u32,
    field72_0x68: u16,
    field73_0x6a: u16,
    field74_0x6c: u32,
    field75_0x70: u32,
    field76_0x74: u32,
    field77_0x78: u32,
    field78_0x7c: bool,
    field79_0x7d: u8,
    field80_0x7e: u8,
    field81_0x7f: u8,
    field82_0x80: i32,
    field83_0x84: [u8; 0x64],
}

#[skyline::hook(offset = FIGHTER_SELECTED_OFFSET, inline)]
fn css_fighter_selected(ctx: &InlineCtx) {
    let infos = unsafe { &*(ctx.registers[0].bindgen_union_field as *const FighterInfo) };
    let infosbasic = unsafe { &*(ctx.registers[0].bindgen_union_field as *const FighterInfoBasic) };
    let fighter_id = infos.fighter_id as i32;
    let skin = infos.fighter_slot as u32;
    let character = kind_to_char(fighter_id) as u32;
    let port = (infosbasic.field77_0x78 & 0xFFFF) as usize;
    println!("character {}\nskin {}\nport {}\n ", character, skin, port);
    GAME_INFO.players[port].character.store(character, Ordering::SeqCst);
    GAME_INFO.players[port].skin.store(skin, Ordering::SeqCst);
}

fn search_offsets() {
    unsafe {
        let text_ptr = getRegionAddress(Region::Text) as *const u8;
        let text_size = (getRegionAddress(Region::Rodata) as usize) - (text_ptr as usize);
        let text = std::slice::from_raw_parts(text_ptr, text_size);

        if let Some(offset) = find_subsequence(text, FIGHTER_SELECTED_SEARCH_CODE) {
            FIGHTER_SELECTED_OFFSET = offset;
        } else {
            println!("Error: no offset found for 'css_fighter_selected'. Defaulting to 13.0.1 offset. This likely won't work.");
        }
    }
}

#[skyline::hook(offset = 0x23344e4, inline)]
unsafe fn selected_stage(ctx: &InlineCtx) {
    println!("stage has been selected");
    GAME_INFO.is_results_screen.store(false, Ordering::SeqCst);
}

extern "C" {
    #[link_name = "_ZN3app24FighterSpecializer_Brave23special_lw_open_commandERNS_7FighterE"]
    fn special_lw_open_command();
    #[link_name = "_ZN3app24FighterSpecializer_Brave23special_lw_close_windowERNS_7FighterEbbb"]
    fn special_lw_close_window(fighter: *mut app::Fighter, arg2: bool, no_decide: bool, arg4: bool);
    #[link_name = "_ZN3app24FighterSpecializer_Brave25special_lw_decide_commandERNS_7FighterENS_28FighterBraveSpecialLwCommandEi"]
    fn special_lw_decide_command(fighter: *mut app::Fighter, command: app::FighterBraveSpecialLwCommand, idx: i32);
    #[link_name = "_ZN3app24FighterSpecializer_Brave23special_lw_select_indexERNS_7FighterEi"]
    fn special_lw_select_index(fighter: *mut app::Fighter, index: i32);
}

#[skyline::hook(replace = special_lw_open_command)]
pub unsafe fn special_lw_open_command_hook(fighter: &mut app::Fighter) {
    let module_accessor: *mut app::BattleObjectModuleAccessor = app::sv_battle_object::module_accessor(*(((&mut (fighter.battle_object) as *mut app::BattleObject) as u64 + 8) as *mut u32)); //this is a mess
    let entry_id = WorkModule::get_int(module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID) as i32;
    let player_num = entry_id as usize;

    GAME_INFO.players[player_num].hero_menu_open.store(true, Ordering::SeqCst);
    GAME_INFO.players[player_num].hero_menu_selection.store(0, Ordering::SeqCst);

    call_original!(fighter);
}

#[skyline::hook(replace = special_lw_select_index)]
pub unsafe fn special_lw_select_index_hook(fighter: &mut app::Fighter, index: i32) {
    let module_accessor: *mut app::BattleObjectModuleAccessor = app::sv_battle_object::module_accessor(*(((&mut (fighter.battle_object) as *mut app::BattleObject) as u64 + 8) as *mut u32)); //this is a mess
    let entry_id = WorkModule::get_int(module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID) as i32;
    let player_num = entry_id as usize;

    GAME_INFO.players[player_num].hero_menu_selection.store(index as u32, Ordering::SeqCst);
    call_original!(fighter, index);
}

#[skyline::hook(replace = special_lw_decide_command)]
pub unsafe fn special_lw_decide_command_hook(fighter: &mut app::Fighter, command: app::FighterBraveSpecialLwCommand, idx: i32) {
    let module_accessor: *mut app::BattleObjectModuleAccessor = app::sv_battle_object::module_accessor(*(((&mut (fighter.battle_object) as *mut app::BattleObject) as u64 + 8) as *mut u32)); //this is a mess
    let entry_id = WorkModule::get_int(module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID) as i32;
    let player_num = entry_id as usize;

    GAME_INFO.players[player_num].hero_menu_selection.store(idx as u32, Ordering::SeqCst);
    GAME_INFO.players[player_num].hero_menu_selected.store(true, Ordering::SeqCst);

    call_original!(fighter, command, idx);
}

#[skyline::hook(replace = special_lw_close_window)]
pub unsafe fn special_lw_close_window_hook(fighter: &mut app::Fighter, arg2: bool, no_decide: bool, arg4: bool) {
    let module_accessor: *mut app::BattleObjectModuleAccessor = app::sv_battle_object::module_accessor(*(((&mut (fighter.battle_object) as *mut app::BattleObject) as u64 + 8) as *mut u32)); //this is a mess
    let entry_id = WorkModule::get_int(module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID) as i32;
    let player_num = entry_id as usize;

    
    GAME_INFO.players[player_num].hero_menu_selected.store(false, Ordering::SeqCst);
    GAME_INFO.players[player_num].hero_menu_open.store(false, Ordering::SeqCst);

    call_original!(fighter, arg2, no_decide, arg4);
}

#[skyline::main(name = "discord_server")]
pub fn main() {
    search_offsets();
    skyline::nro::add_hook(nro_main).unwrap();
    unsafe {
        PLAYER_SAVE_ADDRESS = offset_to_addr(PLAYER_SAVE_OFFSET) as *const u64;
        skyline::nn::ro::LookupSymbol(
            &mut FIGHTER_MANAGER_ADDR,
            "_ZN3lib9SingletonIN3app14FighterManagerEE9instance_E\u{0}".as_bytes().as_ptr(),
        );
        let text_ptr = getRegionAddress(Region::Text) as *const u8;
        let text_size = (getRegionAddress(Region::Rodata) as usize) - (text_ptr as usize);
        let text = std::slice::from_raw_parts(text_ptr, text_size);
        if let Some(offset) = find_subsequence(text, OFFSET1_SEARCH_CODE) {
            OFFSET1 = offset + 0x38;
        }
        if let Some(offset) = find_subsequence(text, OFFSET2_SEARCH_CODE) {
            OFFSET2 = offset - 0xc;
        }
        if let Some(offset) = find_subsequence(text, OFFSET3_SEARCH_CODE) {
            OFFSET3 = offset;
        }
    }
    skyline::install_hooks!(
        some_strlen_thing,
        close_arena,
        update_tag_for_player,
        css_fighter_selected,
        selected_stage,
        special_lw_open_command_hook,
        special_lw_close_window_hook,
        special_lw_decide_command_hook,
        special_lw_select_index_hook
    );
    // acmd::add_custom_hooks!(once_per_frame_per_fighter);

    std::thread::spawn(||{
        loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            if let Err(98) = start_server() {
                break
            }
        }
    });
}