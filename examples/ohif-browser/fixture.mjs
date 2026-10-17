// A Part 10 file built entirely from synthetic metadata and a 64x64 intensity ramp.
export const studyUID='2.25.1441',seriesUID='2.25.1442',sopUID='2.25.1443',patientID='EEL-OHIF-144';
const sopClass='1.2.840.10008.5.1.4.1.1.2';
function element(group,tag,vr,value){
 let data=Buffer.isBuffer(value)?value:Buffer.from(String(value),'ascii');
 if(data.length%2)data=Buffer.concat([data,Buffer.from([vr==='UI'?0:32])]);
 const long=['OB','OW','SQ','UN','UT'].includes(vr),header=Buffer.alloc(long?12:8);
 header.writeUInt16LE(group,0);header.writeUInt16LE(tag,2);header.write(vr,4,'ascii');
 if(long)header.writeUInt32LE(data.length,8);else header.writeUInt16LE(data.length,6);
 return Buffer.concat([header,data]);
}
const us=value=>{const b=Buffer.alloc(2);b.writeUInt16LE(value);return b;};
const ul=value=>{const b=Buffer.alloc(4);b.writeUInt32LE(value);return b;};
export function fixture(){
 const pixels=Buffer.alloc(64*64*2);for(let i=0;i<4096;i++)pixels.writeUInt16LE(i,i*2);
 const meta=Buffer.concat([
  element(2,1,'OB',Buffer.from([0,1])),element(2,2,'UI',sopClass),element(2,3,'UI',sopUID),
  element(2,0x10,'UI','1.2.840.10008.1.2.1'),element(2,0x12,'UI','2.25.1440'),element(2,0x13,'SH','EEL_SYNTHETIC'),
 ]);
 const data=[
  [8,0x16,'UI',sopClass],[8,0x18,'UI',sopUID],[8,0x20,'DA','20261017'],[8,0x30,'TM','120000'],[8,0x60,'CS','CT'],[8,0x1030,'LO','Synthetic grayscale ramp'],
  [0x10,0x10,'PN','SYNTHETIC^OHIF'],[0x10,0x20,'LO',patientID],
  [0x18,0x50,'DS','1'],[0x20,0x0d,'UI',studyUID],[0x20,0x0e,'UI',seriesUID],[0x20,0x10,'SH','SYNTHETIC144'],[0x20,0x11,'IS','1'],[0x20,0x13,'IS','1'],
  [0x20,0x32,'DS','0\\0\\0'],[0x20,0x37,'DS','1\\0\\0\\0\\1\\0'],[0x20,0x52,'UI','2.25.1444'],
  [0x28,2,'US',us(1)],[0x28,4,'CS','MONOCHROME2'],[0x28,0x10,'US',us(64)],[0x28,0x11,'US',us(64)],[0x28,0x30,'DS','1\\1'],
  [0x28,0x100,'US',us(16)],[0x28,0x101,'US',us(12)],[0x28,0x102,'US',us(11)],[0x28,0x103,'US',us(0)],
  [0x28,0x1050,'DS','2048'],[0x28,0x1051,'DS','4096'],[0x28,0x1052,'DS','0'],[0x28,0x1053,'DS','1'],[0x7fe0,0x10,'OW',pixels],
 ];
 return {pixels,file:Buffer.concat([Buffer.alloc(128),Buffer.from('DICM'),element(2,0,'UL',ul(meta.length)),meta,...data.map(e=>element(...e))])};
}
