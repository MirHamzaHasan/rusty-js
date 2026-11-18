mod common;

fn succeeds(body: &str, expected: &[u8]) {
    let output = common::run(&["-e", body]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected);
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn global_esm_commonjs_and_aliases_share_constructor_identity() {
    succeeds(
        "Promise.all([import('node:buffer'),import('buffer'),import('rustyjs:buffer')]).then(async ([a,b,c])=>{if(a!==b||b!==c||a.Buffer!==Buffer)throw Error('ESM identity');const {createRequire}=await import('rustyjs:module');const require=createRequire('/virtual.cjs');const object=require('buffer');if(object!==require('node:buffer')||object!==require('rustyjs:buffer')||object.Buffer!==Buffer||require.resolve('node:buffer')!=='node:buffer')throw Error('CJS identity');const Original=Buffer;globalThis.Buffer=null;if((await import('buffer')).Buffer!==Original)throw Error('global replacement');console.log('buffer identities');});",
        b"buffer identities\n",
    );
}

#[test]
fn copies_and_shared_views_have_distinct_ownership() {
    succeeds(
        "const input=new Uint8Array([1,2,3,4]);const copy=Buffer.from(input);input[0]=9;if(copy[0]!==1)throw Error('copy aliases');const shared=Buffer.from(input.buffer,1,2);shared[0]=7;if(input[1]!==7)throw Error('ArrayBuffer not shared');const slice=copy.slice(1,3),sub=copy.subarray(-2);slice[0]=8;sub[1]=6;if(copy.toString('hex')!=='01080306'||slice.byteOffset!==copy.byteOffset+1||!Buffer.isBuffer(slice)||!(copy instanceof Uint8Array))throw Error('views');const deep=Buffer.from(copy);copy[0]=0;if(deep[0]!==1)throw Error('Buffer copy');const words=Buffer.from(new Uint16Array([0x1234,0x1ff]));if(words.toString('hex')!=='34ff')throw Error('element conversion');console.log('buffer ownership');",
        b"buffer ownership\n",
    );
}

#[test]
fn utf8_handles_multibyte_surrogates_nuls_and_malformed_sequences() {
    succeeds(
        "const text='Aé💩\\u0000';const b=Buffer.from(text);if(b.toString('hex')!=='41c3a9f09f92a900'||b.toString()!==text||Buffer.byteLength(text)!==8)throw Error('UTF8 roundtrip');if(Buffer.from('\\ud800x\\udc00').toString('hex')!=='efbfbd78efbfbd')throw Error('lone surrogate');for(const [hex,text] of [['e282','�'],['e228a1','�(�'],['f0808080','����'],['eda080','���'],['c0af','��']])if(Buffer.from(hex,'hex').toString()!==text)throw Error('malformed '+hex);console.log('buffer utf8');",
        b"buffer utf8\n",
    );
}

#[test]
fn hex_and_base64_decode_partial_and_malformed_input_deliberately() {
    succeeds(
        "for(const [text,hex] of [['1a7','1a'],['1ag123','1a'],['xx',''],['00ffAB','00ffab']])if(Buffer.from(text,'hex').toString('hex')!==hex)throw Error('hex '+text);for(const [text,hex] of [[' Z m\\n9v!! ','666f6f'],['Zg','66'],['Z',''],['Zg==junk','66'],['-_8','fbff'],['+/8=','fbff']])for(const enc of ['base64','base64url'])if(Buffer.from(text,enc).toString('hex')!==hex)throw Error('base64 '+text);for(const text of ['','f','fo','foo','foob','fooba','foobar']){const bytes=Buffer.from(text);if(Buffer.from(bytes.toString('base64'),'base64').toString()!==text||Buffer.from(bytes.toString('base64url'),'base64url').toString()!==text)throw Error('base64 roundtrip');}if(Buffer.from([251,255]).toString('base64')!=='+/8='||Buffer.from([251,255]).toString('base64url')!=='-_8')throw Error('alphabet');console.log('buffer encodings');",
        b"buffer encodings\n",
    );
}

#[test]
fn single_byte_and_utf16_encodings_preserve_the_documented_bits() {
    succeeds(
        "if(Buffer.from('éÿĀ','latin1').toString('hex')!=='e9ff00'||Buffer.from('é','ascii')[0]!==233||Buffer.from([233,255]).toString('ascii')!=='i\\u007f')throw Error('single byte');const text='A💩\\ud800';if(Buffer.from(text,'ucs2').toString('hex')!=='41003dd8a9dc00d8'||Buffer.from(text,'UTF-16LE').toString('utf16le')!==text||Buffer.from([65,0,66]).toString('utf16le')!=='A')throw Error('utf16');for(const encoding of ['UTF-8','hex','base64url','binary','ucs-2'])if(!Buffer.isEncoding(encoding))throw Error('isEncoding');for(const value of ['',null,42,'wat'])if(Buffer.isEncoding(value))throw Error('invalid encoding');console.log('buffer character encodings');",
        b"buffer character encodings\n",
    );
}

#[test]
fn allocation_fill_and_overlapping_copy_are_correct() {
    succeeds(
        "for(const allocate of [Buffer.alloc,Buffer.allocUnsafe,Buffer.allocUnsafeSlow])if(allocate(3).toString('hex')!=='000000')throw Error('allocation not zeroed');const b=Buffer.alloc(6,'ab');if(b.toString()!=='ababab'||b.fill('ff',1,3,'hex')!==b||b.toString('hex')!=='61ffff626162')throw Error('fill');const overlap=Buffer.from([1,2,3,4]);if(overlap.copy(overlap,1,0,3)!==3||overlap.toString('hex')!=='01010203')throw Error('overlap copy');const target=new Uint8Array(2);if(overlap.copy(target,0,1)!==2||target[0]!==1||target[1]!==2)throw Error('truncating copy');const pattern=Buffer.from([1,2,3,4]);pattern.fill(pattern.subarray(0,2),1);if(pattern.toString('hex')!=='01010201')throw Error('alias fill');if(Buffer.alloc(3,'').toString('hex')!=='000000')throw Error('empty fill');console.log('buffer writes');",
        b"buffer writes\n",
    );
}

#[test]
fn string_writes_respect_offsets_and_never_split_encoded_characters() {
    succeeds(
        "const b=Buffer.alloc(8,46);if(b.write('é💩',1,4)!==2||b.toString('hex')!=='2ec3a92e2e2e2e2e')throw Error('partial UTF8');if(b.write('💩',3,3)!==0||b.write('é',3,1)!==0||b.write('AB',3,'utf16le')!==4||b.toString('hex')!=='2ec3a9410042002e')throw Error('write overload');if(Buffer.alloc(1).write('x',0,1,'utf16le')!==0)throw Error('partial UTF16');if(Buffer.alloc(0).write('é')!==0)throw Error('zero destination');if(Buffer.alloc(3).write('abcd','hex')!==2)throw Error('hex write');console.log('buffer string writes');",
        b"buffer string writes\n",
    );
}

#[test]
fn concat_comparison_json_and_typed_array_iteration() {
    succeeds(
        "const a=Buffer.from([1,2]),b=Buffer.from([3]);if(Buffer.concat([a,b],5).toString('hex')!=='0102030000'||Buffer.concat([a,b],1)[0]!==1||Buffer.concat([]).length!==0)throw Error('concat');if(Buffer.compare(a,new Uint8Array([1,2]))!==0||a.compare(b)!==-1||Buffer.compare(b,a)!==1||!a.equals(Buffer.from([1,2])))throw Error('compare');const json=JSON.stringify(a);if(json!=='{\"type\":\"Buffer\",\"data\":[1,2]}'||!Buffer.from(JSON.parse(json)).equals(a)||[...a].join(',')!=='1,2')throw Error('JSON/iteration');if(Buffer.isBuffer(new Uint8Array(1))||Buffer.isBuffer(Object.create(Buffer.prototype))||Buffer.isBuffer(null))throw Error('brand');if(Buffer.byteLength(new Uint16Array(3))!==6||Buffer.byteLength(new ArrayBuffer(4))!==4)throw Error('byteLength');console.log('buffer utility methods');",
        b"buffer utility methods\n",
    );
}

#[test]
fn invalid_sizes_offsets_encodings_and_receivers_throw_before_mutation() {
    succeeds(
        "const b=Buffer.from([1,2]);const actions=[()=>Buffer.alloc(-1),()=>Buffer.alloc(1.5),()=>Buffer.alloc(NaN),()=>Buffer.alloc(Infinity),()=>Buffer.alloc(67108865),()=>Buffer.from(new ArrayBuffer(2),1,2),()=>b.copy(b,-1),()=>b.fill(3,0,3),()=>b.write('x',3),()=>b.write('x',0,-1)];for(const action of actions){try{action();throw Error('bad bounds accepted');}catch(error){if(!(error instanceof RangeError))throw error;}}for(const action of [()=>Buffer.alloc('2'),()=>new Buffer(2),()=>Buffer.from(2),()=>Buffer.from('x','bad'),()=>b.fill('zz',0,2,'hex'),()=>Buffer.compare(b,[]),()=>Buffer.prototype.toString.call(new Uint8Array(2))]){try{action();throw Error('bad type accepted');}catch(error){if(!(error instanceof TypeError))throw error;}}if(b.toString('hex')!=='0102')throw Error('partial mutation');if(b.slice(2,1).length!==0||b.slice(-Infinity,Infinity).length!==2||b.toString('hex',-2,Infinity)!=='0102')throw Error('clamping');console.log('buffer validation');",
        b"buffer validation\n",
    );
}

#[test]
fn resizable_shared_and_detached_backing_stores_do_not_enter_native_conversion() {
    succeeds(
        "for(const value of [new ArrayBuffer(2,{maxByteLength:4}),new SharedArrayBuffer(2)]){try{Buffer.from(value);throw Error('unsupported backing store');}catch(error){if(!(error instanceof TypeError))throw error;}}const b=Buffer.from([1,2]);b.buffer.transfer();for(const action of [()=>b.toString(),()=>b.subarray(),()=>b.fill(1)]){try{action();throw Error('detached accepted');}catch(error){if(!(error instanceof TypeError))throw error;}}console.log('buffer backing stores');",
        b"buffer backing stores\n",
    );
}

#[test]
fn fixed_width_numeric_io_checks_endianness_bounds_and_value_types() {
    succeeds(
        "const b=Buffer.alloc(24);if(b.writeUInt32BE(0x12345678,1)!==5||b.toString('hex',1,5)!=='12345678'||b.readUInt32LE(1)!==0x78563412)throw Error('endianness');b.writeInt16LE(-123,6);if(b.readInt16LE(6)!==-123)throw Error('signed');b.writeBigUInt64BE(18446744073709551615n,8);if(b.readBigUint64LE(8)!==18446744073709551615n)throw Error('u64');b.writeBigInt64LE(-9223372036854775808n,8);if(b.readBigInt64LE(8)!==-9223372036854775808n)throw Error('i64');b.writeDoubleBE(Math.PI,16);if(b.readDoubleBE(16)!==Math.PI)throw Error('double');b.writeFloatLE(Infinity,16);if(b.readFloatLE(16)!==Infinity)throw Error('float');if(b.readUint32BE!==b.readUInt32BE)throw Error('alias');for(const action of [()=>b.writeUInt8(256),()=>b.writeInt8(-129),()=>b.writeUInt32LE(1.5),()=>b.readDoubleBE(17),()=>b.readUInt8(-1),()=>b.writeBigInt64BE(9223372036854775808n)]){try{action();throw Error('numeric bounds accepted');}catch(error){if(!(error instanceof RangeError))throw error;}}for(const action of [()=>b.writeUInt8('1'),()=>b.writeBigUInt64LE(1),()=>b.readUInt8('0')]){try{action();throw Error('numeric type accepted');}catch(error){if(!(error instanceof TypeError))throw error;}}console.log('buffer numeric IO');",
        b"buffer numeric IO\n",
    );
}

#[test]
fn captured_intrinsics_and_real_view_lengths_survive_guest_replacement() {
    succeeds(
        "const b=Buffer.from([1,2,3]);Object.defineProperty(b,'byteLength',{value:999});if(Buffer.byteLength(b)!==3)throw Error('shadow byteLength');Uint8Array.prototype.set=()=>{throw Error('patched set');};Uint8Array.prototype.fill=()=>{throw Error('patched fill');};WeakSet.prototype.has=()=>{throw Error('patched brand');};ArrayBuffer.isView=()=>false;DataView.prototype.getUint32=()=>{throw Error('patched numeric');};String.prototype.charCodeAt=()=>{throw Error('patched encoding');};String.prototype.toLowerCase=()=>{throw Error('patched casing');};const bytes=Buffer.alloc(4,'ff','hex');if(bytes.readUInt32BE()!==4294967295||Buffer.byteLength(b)!==3||Buffer.from('YQ','BASE64').toString()!=='a'||b.slice(1).toString('hex')!=='0203')throw Error('captured methods');console.log('buffer intrinsics');",
        b"buffer intrinsics\n",
    );
}
