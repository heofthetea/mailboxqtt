/// A collection of constants for fixed protocol messages

pub const CONNACK: [u8; 4] =  [
    // Fixed header
    0x20, // CONNACK Packet type
    0x02, // Remaining length
    // Variable header
    0x00, // Connection accepted
    0x00, // Connection accepted
];
