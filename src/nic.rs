extern crate alloc;
use core::mem::size_of;
use crate::info;
use crate::pci::BusDeviceFunction;
use crate::pci::VendorDeviceId;


use crate::pci::Pci;
use crate::result::Result;

pub trait NicDevice {
    fn initialize(&mut self, accept_all: bool);
    fn send<T>(&mut self, buf: *const T, length: u16) -> u8;
    fn receive<T>(&mut self, buf: *const T) -> u16;
}

pub struct NicDriver {
    nic: Box<dyn NicDevice>,
}

impl NicDriver {
    pub fn supports(vp: VendorDeviceId) -> bool {
        const VDI_LIST: [VendorDeviceId; 1] = [
            // NIC Intel e1000e
            VendorDeviceId {
                vendor: 0x8086,
                device: 0x10D3, 
            },
        ];
        VDI_LIST.contains(&vp)
    }

    pub fn attach(pci: &Pci, bdf: BusDeviceFunction) -> Result<()> {

        info!("Nic found at: {bdf:?}");
        pci.disable_interrupt(bdf)?;
        pci.enable_bus_master(bdf)?;

        //bar
        let bar0 = pci.try_bar0_mem64(bdf)?;
        info!("[BAR0_NIC]{:?}", bar0);
        bar0.disable_cache();
        info!("[NIC] bar0.addr():{:?}", bar0.addr());
        let mmio_base = bar0.addr() as u64;
        info!("[NIC] mmio_base:{}", mmio_base);

        // MEMO pciで既に確認しているので冗長か
        let Some(vd) = pci.read_vendor_id_and_device_id(bdf) else {
            return Err("Failed to read Vendor ID and Device ID");
        }

        // FIXME もう少しスマートな書き方がありそう
        if ( vd.vendor, vd.device) == (0x8086, 0x10D3) {
            let mut nic = NicDeviceE1000E::new(mmio_base);
            nic.initialize(true);
            self.nic = Box::new(nic);
        }
        else {
            return Err("Unsupported NIC device");
        }

        // let regs: = nic::write_register;
        // spawn_global(Self::run());

        // [TEST] SEND [START]
        let message_buffer = "Hello, World!";
        nic.send_str(message_buffer, message_buffer.len() as u16);
        // [TEST] SEND [END]

        Ok(())
    }
}


#[derive(Debug)]
pub struct NicDeviceE1000E {
    mmio_base: u64,
    t_descriptor: *mut TDescriptor,
    r_descriptor: *mut RDescriptor,
    packet_buffer: u64,
    t_tail: u64,
    r_tail: u64,
}

static mut T_DESC_RING_BUFFER: [TDescriptor; T_DESC_NUM] = [TDescriptor::new(); T_DESC_NUM];
static mut R_DESC_RING_BUFFER: [RDescriptor; R_DESC_NUM] = [RDescriptor::new(); R_DESC_NUM];

const PACKET_SIZE: usize  = 2048;
const T_DESC_CMD_RSV: u8  = 0b00001000;
const T_DESC_CMD_EOP: u8  = 0b1;

const CTRL_OFFSET :u16    = 0x00000;
const CTRL_FD: u16        = 0x00000001;
const CTRL_ASDE: u16      = 0x000000020;
const CTRL_SLU: u16       = 0x000000040;

const TCTL_OFFSET: u16    = 0x00400;
const TIPG_OFFSET: u16    = 0x00410;
const TDBAL_OFFSET: u16   = 0x03800;
const TDBAH_OFFSET: u16   = 0x03804;
const TDLEN_OFFSET: u16   = 0x03808;
const TDH_OFFSET: u16     = 0x03810;
const TDT_OFFSET: u16     = 0x03818;
const TCTL_EN: u32        = 0x00000002;
const TCTL_PSP: u32       = 0x00000008;
const TCTL_CT: u32        = 0x00000100;
const TCTL_COLD: u32      = 0x00400000;
const TIPG_IPGT: u16      = 8;
const TIPG_IPGR1: u16     = 8;
const TIPG_IPGR2: u16     = 6;

const RCTL_OFFSET: u16    = 0x00100;
const RDBAL_OFFSET: u16   = 0x02800;
const RDBAH_OFFSET: u16   = 0x02804;
const RDLEN_OFFSET: u16   = 0x02808;
const RDH_OFFSET: u16     = 0x02810;
const RDT_OFFSET: u16     = 0x02818;
const RDTR_OFFSET: u16    = 0x02820;
const RADV_OFFSET: u16    = 0x0282C;
const RAL_OFFSET : u16    = 0x05400;
const RAH_OFFSET: u16     = 0x05404;
const RCTL_EN: u32        = 0x00000002;
const RCTL_BAM: u32       = 0x00008000;
const RDTR_DELAY: u32     = 0x00001000;
const RADV_DELAY: u32     = 0x00001000;

impl NicDevice for NicDeviceE1000E {

    pub fn new(mmio_base: u64) -> Self {
        let t_desc: *mut TDescriptor;
        let r_desc: *mut RDescriptor;
        unsafe {
            t_desc = T_DESC_RING_BUFFER.as_mut_ptr();
            r_desc = R_DESC_RING_BUFFER.as_mut_ptr();
        };
        info!("[NIC] t_desc_ring_buffer:{:?}", t_desc);
        info!("[NIC] r_desc_ring_buffer:{:?}", r_desc);

        Nic { mmio_base, t_descriptor: t_desc, r_descriptor: r_desc, packet_buffer: 0, t_tail: 0, r_tail: 0 }
    }

    pub fn initialize(&mut self, accept_all: bool) {
        
        unsafe {
            self.t_descriptor = T_DESC_RING_BUFFER.as_mut_ptr();
            self.r_descriptor = R_DESC_RING_BUFFER.as_mut_ptr();

            info!("[NIC] t_desc = {:?}", self.t_descriptor);
            info!("[NIC] r_desc = {:?}", self.r_descriptor);

            for i in 0..T_DESC_NUM {

                T_DESC_RING_BUFFER[i].buffer_address = 0;
                T_DESC_RING_BUFFER[i].length = 0;
                T_DESC_RING_BUFFER[i].checksum_offset = 0;
                T_DESC_RING_BUFFER[i].command = T_DESC_CMD_RSV;
                T_DESC_RING_BUFFER[i].status = 0;
                T_DESC_RING_BUFFER[i].reserved = 0;
                T_DESC_RING_BUFFER[i].checksum_start_field = 0;
                T_DESC_RING_BUFFER[i].special = 0;

            }

            self.packet_buffer = [[0; PACKET_SIZE]; R_DESC_NUM].as_ptr() as u64;

            for i in 0..R_DESC_NUM {
                info!("[NIC] RDESC index = {i}");
                R_DESC_RING_BUFFER[i].buffer_address = (self.packet_buffer as *const u64).add(i * PACKET_SIZE) as u64;
                R_DESC_RING_BUFFER[i].status = 0;
                R_DESC_RING_BUFFER[i].errors = 0;
                info!("[NIC] R_DESC_RING_BUFFER = {:?}", R_DESC_RING_BUFFER[i]);
            }

        }

        self.write_register(CTRL_OFFSET, (CTRL_FD | CTRL_ASDE | CTRL_SLU) as u32);

        self.t_tail = 0;
        self.r_tail = R_DESC_NUM as u64  - 1;

        self.write_register(TCTL_OFFSET, (TCTL_EN | TCTL_PSP | TCTL_CT | TCTL_COLD) as u32);
        self.write_register(TIPG_OFFSET, (TIPG_IPGT | TIPG_IPGR1 | TIPG_IPGR2) as u32);
        self.write_register(TDBAL_OFFSET, self.t_descriptor as u32);
        self.write_register(TDBAH_OFFSET, (self.t_descriptor as u64 >> 32) as u32);
        self.write_register(TDLEN_OFFSET, (size_of::<TDescriptor>() * T_DESC_NUM) as u32);
        self.write_register(TDH_OFFSET, self.t_tail as u32);
        self.write_register(TDT_OFFSET, self.t_tail as u32);

        let rctl_value = RCTL_BAM | RCTL_EN;
        self.write_register(RCTL_OFFSET, rctl_value);
        self.write_register(RDTR_OFFSET, RDTR_DELAY);
        self.write_register(RADV_OFFSET, RADV_DELAY);
        self.write_register(RDBAL_OFFSET, self.r_descriptor as u32);
        self.write_register(RDBAH_OFFSET, (self.r_descriptor as u64 >> 32) as u32);
        self.write_register(RDLEN_OFFSET, (size_of::<RDescriptor>() * R_DESC_NUM) as u32);
        self.write_register(RDH_OFFSET, 0);
        self.write_register(RDT_OFFSET, self.r_tail as u32);

    }

    fn read_register(&self, offset: u16) -> u32 {
        let reg_addr = offset + (self.mmio_base as u16);
        reg_addr as u32
    }

    fn write_register(&self, offset: u16, value: u32) {
        info!("{:?}", self.mmio_base);
        let reg_addr = (offset as u64 + (self.mmio_base)) as *mut u32;
        info!("[NIC] {offset}, {:04X}, {:?}", value, reg_addr);
        unsafe {
            *reg_addr = value;
        };
        info!("[NIC] *reg_addr={:?}", &reg_addr);
    }

    pub fn send<T>(&mut self, buf: *const T, length: u16) -> u8 {
        let mut send_status = 0;
        unsafe {
            let desc = self.t_descriptor.add(self.t_tail as usize);
            let mut desc_ref = *desc;
            desc_ref.buffer_address = buf as u64;
            desc_ref.length = length;
            desc_ref.command = desc_ref.command | T_DESC_CMD_EOP;
            desc_ref.status = 0;

            self.t_tail = self.t_tail + 1;
            self.write_register(TDT_OFFSET, self.t_tail as u32);

            while send_status == 0 {
                send_status = desc_ref.status & 0x0f;
            }
        }
        send_status
    }

    pub fn send_str(&mut self, buf: &str, length: u16) -> u8 {
        let mut send_status = 0;
        unsafe {
            let desc = self.t_descriptor.add(self.t_tail as usize);
            let mut desc_ref = *desc;
            desc_ref.buffer_address = buf.as_ptr() as u64;
            desc_ref.length = length;
            desc_ref.command = desc_ref.command | T_DESC_CMD_EOP;
            desc_ref.status = 0;

            self.t_tail = self.t_tail + 1;
            self.write_register(TDT_OFFSET, self.t_tail as u32);

            while send_status == 0 {
                send_status = desc_ref.status & 0x0f;
            }
        }
        send_status
    }

}

const T_DESC_NUM:usize = 8;
const R_DESC_NUM:usize = 16;

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug)]
pub struct TDescriptor {
    buffer_address: u64,
    length: u16,
    checksum_offset: u8,
    command: u8,
    status: u8, //4
    reserved: u8, //4
    checksum_start_field: u8,
    special: u16,
}

impl TDescriptor {
    pub const fn new() -> Self {
        TDescriptor { 
            buffer_address: 0, 
            length: 0, 
            checksum_offset: 0, 
            command: 0, 
            status: 4, 
            reserved: 4, 
            checksum_start_field: 0, 
            special: 0, 
        }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug)]
struct RDescriptor {
    buffer_address: u64,
    length: u16,
    reserved: u16,
    status: u8,
    errors: u8,
    special: u16,
}
impl RDescriptor {
    pub const fn new() -> Self {
        RDescriptor { 
            buffer_address: 0, 
            length: 0, 
            reserved: 4,  
            status: 4, 
            errors: 0,
            special: 0, 
        }
    }
}

